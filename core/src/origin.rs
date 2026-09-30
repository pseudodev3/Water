use crate::{config::Config, model::{Chain, ScanRequest}};
use num_bigint::BigUint;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize)]
pub struct OriginRequest {
    pub chain: Chain,
    pub token: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LaunchpadEvidence {
    pub name: String,
    pub family: String,
    pub evidence: String,
    pub source: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct OriginResponse {
    pub chain: Chain,
    pub token: String,
    pub primary_label: String,
    pub primary_address: Option<String>,
    pub secondary_label: Option<String>,
    pub secondary_address: Option<String>,
    pub primary_balance_percentage: Option<f64>,
    pub creator_label: Option<String>,
    pub launchpad: Option<LaunchpadEvidence>,
    pub active_controls: Vec<String>,
    pub source: String,
    pub detail: String,
}

pub async fn inspect_origin(
    http: Client,
    config: &Config,
    request: OriginRequest,
) -> Result<OriginResponse, String> {
    ScanRequest {
        chain: request.chain,
        address: request.token.clone(),
    }
    .validate()?;

    let token = request.token.trim().to_string();

    match request.chain {
        Chain::Solana => inspect_solana(&http, config, &token).await,
        Chain::Robinhood => inspect_robinhood(&http, config, &token).await,
    }
}

async fn inspect_solana(
    http: &Client,
    config: &Config,
    token: &str,
) -> Result<OriginResponse, String> {
    let account = solana_rpc_with_fallback(
        http,
        &config.solana_rpc_url,
        &config.solana_fallback_rpc_url,
        "getAccountInfo",
        json!([token, {"encoding": "jsonParsed", "commitment": "confirmed"}]),
    )
    .await?;

    let info = account
        .pointer("/value/data/parsed/info")
        .ok_or_else(|| "Solana mint account did not expose parsed mint authority data.".to_string())?;

    let mint_authority = info
        .get("mintAuthority")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let freeze_authority = info
        .get("freezeAuthority")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let mut active_controls = Vec::new();
    if mint_authority.is_some() {
        active_controls.push("Mint authority is still active".to_string());
    }
    if freeze_authority.is_some() {
        active_controls.push("Freeze authority is still active".to_string());
    }
    if active_controls.is_empty() {
        active_controls.push("Standard mint and freeze authorities are revoked".to_string());
    }

    let authority_balance_percentage = match mint_authority.as_deref() {
        Some(authority) => {
            let (accounts, supply) = tokio::join!(
                solana_rpc_with_fallback(
                    http,
                    &config.solana_rpc_url,
                    &config.solana_fallback_rpc_url,
                    "getTokenAccountsByOwner",
                    json!([
                        authority,
                        {"mint": token},
                        {"encoding": "jsonParsed", "commitment": "confirmed"}
                    ]),
                ),
                solana_rpc_with_fallback(
                    http,
                    &config.solana_rpc_url,
                    &config.solana_fallback_rpc_url,
                    "getTokenSupply",
                    json!([token, {"commitment": "confirmed"}]),
                )
            );

            match (accounts, supply) {
                (Ok(accounts), Ok(supply)) => {
                    let held = accounts
                        .get("value")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(|row| {
                                    row.pointer("/account/data/parsed/info/tokenAmount/uiAmountString")
                                        .and_then(Value::as_str)
                                        .and_then(|value| value.parse::<f64>().ok())
                                })
                                .sum::<f64>()
                        })
                        .unwrap_or(0.0);
                    let total = supply
                        .pointer("/value/uiAmountString")
                        .and_then(Value::as_str)
                        .and_then(|value| value.parse::<f64>().ok());
                    total.filter(|value| *value > 0.0)
                        .map(|total| (held / total * 100.0).clamp(0.0, 100.0))
                }
                _ => None,
            }
        }
        None => None,
    };

    let launchpad = detect_solana_launchpad(http, config, token).await;

    Ok(OriginResponse {
        chain: Chain::Solana,
        token: token.to_string(),
        primary_label: "Mint authority".to_string(),
        primary_address: mint_authority,
        secondary_label: Some("Freeze authority".to_string()),
        secondary_address: freeze_authority,
        primary_balance_percentage: authority_balance_percentage,
        creator_label: None,
        launchpad,
        active_controls,
        source: "Solana parsed mint state".to_string(),
        detail: "Water shows the standard SPL mint and freeze authorities only. Token-2022 extension authorities are not inferred here, and unrelated wallets are never labeled as insiders without an onchain relationship.".to_string(),
    })
}

async fn inspect_robinhood(
    http: &Client,
    config: &Config,
    token: &str,
) -> Result<OriginResponse, String> {
    let key = config
        .blockscout_api_key
        .as_ref()
        .ok_or_else(|| "BLOCKSCOUT_API_KEY is required for Robinhood origin indexing.".to_string())?;

    let url = format!(
        "{}/addresses/{}",
        config.blockscout_api_url.trim_end_matches('/'),
        token
    );
    let response = http
        .get(url)
        .header("Accept", "application/json")
        .header("User-Agent", "water/0.1")
        .query(&[("apikey", key)])
        .send()
        .await
        .map_err(|error| format!("Blockscout origin lookup failed: {error}"))?;

    let status = response.status();
    let body = response
        .json::<Value>()
        .await
        .map_err(|error| format!("Blockscout origin lookup returned unreadable JSON: {error}"))?;

    if !status.is_success() {
        return Err(format!(
            "Blockscout origin lookup returned HTTP {}.",
            status.as_u16()
        ));
    }

    let payload = body.get("data").unwrap_or(&body);
    let creator = payload
        .get("creator_address_hash")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let creation_tx = payload
        .get("creation_transaction_hash")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let implementation = payload
        .get("implementation_address")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let creator_metadata = match creator.as_deref() {
        Some(address) => blockscout_address(http, config, address).await.ok(),
        None => None,
    };
    let creator_label = creator_metadata
        .as_ref()
        .and_then(blockscout_best_label);
    let launchpad = creator_metadata
        .as_ref()
        .and_then(|metadata| recognize_robinhood_launchpad(metadata, creator.as_deref()));

    let creator_balance_percentage = match creator.as_deref() {
        Some(address) => erc20_balance_percentage(http, &config.robinhood_rpc_url, token, address)
            .await
            .ok(),
        None => None,
    };

    let mut active_controls = Vec::new();
    if let Some(address) = creator.as_deref() {
        match evm_rpc(
            http,
            &config.robinhood_rpc_url,
            "eth_getCode",
            json!([address, "latest"]),
        )
        .await
        {
            Ok(code)
                if code
                    .as_str()
                    .is_some_and(|value| value != "0x" && value != "0x0") =>
            {
                active_controls.push("Creator address is a contract".to_string());
            }
            Ok(_) => active_controls.push("Creator address is an EOA wallet".to_string()),
            Err(_) => {}
        }
    }
    if implementation.is_some() {
        active_controls.push("Token address points to a proxy implementation".to_string());
    }
    if active_controls.is_empty() {
        active_controls.push("No additional creator control was proven".to_string());
    }

    Ok(OriginResponse {
        chain: Chain::Robinhood,
        token: token.to_string(),
        primary_label: "Contract creator".to_string(),
        primary_address: creator,
        secondary_label: creation_tx.as_ref().map(|_| "Creation transaction".to_string()),
        secondary_address: creation_tx,
        primary_balance_percentage: creator_balance_percentage,
        creator_label,
        launchpad,
        active_controls,
        source: "Blockscout indexed contract origin + Robinhood JSON-RPC".to_string(),
        detail: match implementation {
            Some(value) => format!(
                "Blockscout also reports implementation {value}. Water does not infer additional insider wallets without direct evidence."
            ),
            None => "Water does not infer additional insider wallets without direct evidence.".to_string(),
        },
    })
}


async fn detect_solana_launchpad(
    http: &Client,
    config: &Config,
    mint: &str,
) -> Option<LaunchpadEvidence> {
    let signatures = solana_rpc_with_fallback(
        http,
        &config.solana_rpc_url,
        &config.solana_fallback_rpc_url,
        "getSignaturesForAddress",
        json!([mint, {"limit": 100}]),
    )
    .await
    .ok()?;

    let rows = signatures.as_array()?;
    let signature = rows
        .last()
        .and_then(|row| row.get("signature"))
        .and_then(Value::as_str)?;

    let transaction = solana_rpc_with_fallback(
        http,
        &config.solana_rpc_url,
        &config.solana_fallback_rpc_url,
        "getTransaction",
        json!([
            signature,
            {
                "encoding": "jsonParsed",
                "commitment": "confirmed",
                "maxSupportedTransactionVersion": 0
            }
        ]),
    )
    .await
    .ok()?;

    let program_ids = solana_transaction_program_ids(&transaction);

    const PROGRAMS: &[(&str, &str, &str)] = &[
        (
            "MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e",
            "Pump.fun Mayhem",
            "Pump.fun",
        ),
        (
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
            "Pump.fun",
            "Pump.fun",
        ),
        (
            "boop8hVGQGqehUK2iVEMEnMrL5RbjywRzHKBmBE7ry4",
            "Boop.fun",
            "Boop.fun",
        ),
        (
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj",
            "Raydium LaunchLab family",
            "Raydium LaunchLab",
        ),
        (
            "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN",
            "Meteora DBC family",
            "Meteora Dynamic Bonding Curve",
        ),
        (
            "MoonCVVNZFSYkqNXP6bxHLPL6QQJiMagDL3qcqUQTrG",
            "Moonshot",
            "Moonshot",
        ),
    ];

    PROGRAMS.iter().find_map(|(program, name, family)| {
        program_ids.iter().any(|seen| seen == program).then(|| LaunchpadEvidence {
            name: (*name).to_string(),
            family: (*family).to_string(),
            evidence: format!("Mint history invoked launch program {program}."),
            source: "Solana transaction program match".to_string(),
        })
    })
}

fn solana_transaction_program_ids(transaction: &Value) -> Vec<String> {
    let mut programs = Vec::new();

    if let Some(keys) = transaction
        .pointer("/transaction/message/accountKeys")
        .and_then(Value::as_array)
    {
        for key in keys {
            let pubkey = key
                .as_str()
                .or_else(|| key.get("pubkey").and_then(Value::as_str));
            if let Some(pubkey) = pubkey {
                programs.push(pubkey.to_string());
            }
        }
    }

    if let Some(instructions) = transaction
        .pointer("/transaction/message/instructions")
        .and_then(Value::as_array)
    {
        collect_program_ids(instructions, &mut programs);
    }

    if let Some(groups) = transaction
        .pointer("/meta/innerInstructions")
        .and_then(Value::as_array)
    {
        for group in groups {
            if let Some(instructions) = group.get("instructions").and_then(Value::as_array) {
                collect_program_ids(instructions, &mut programs);
            }
        }
    }

    programs.sort();
    programs.dedup();
    programs
}

fn collect_program_ids(instructions: &[Value], output: &mut Vec<String>) {
    for instruction in instructions {
        if let Some(program) = instruction.get("programId").and_then(Value::as_str) {
            output.push(program.to_string());
        }
    }
}

async fn blockscout_address(
    http: &Client,
    config: &Config,
    address: &str,
) -> Result<Value, String> {
    let key = config
        .blockscout_api_key
        .as_ref()
        .ok_or_else(|| "BLOCKSCOUT_API_KEY is not configured.".to_string())?;
    let url = format!(
        "{}/addresses/{address}",
        config.blockscout_api_url.trim_end_matches('/')
    );

    let response = http
        .get(url)
        .header("Accept", "application/json")
        .header("User-Agent", "water/0.1")
        .query(&[("apikey", key)])
        .send()
        .await
        .map_err(|error| format!("Blockscout creator metadata failed: {error}"))?;

    let status = response.status();
    let body = response
        .json::<Value>()
        .await
        .map_err(|error| format!("Blockscout creator metadata returned unreadable JSON: {error}"))?;

    if !status.is_success() {
        return Err(format!(
            "Blockscout creator metadata returned HTTP {}.",
            status.as_u16()
        ));
    }

    Ok(body.get("data").cloned().unwrap_or(body))
}

fn blockscout_best_label(value: &Value) -> Option<String> {
    value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("public_tags")
                .and_then(Value::as_array)
                .and_then(|tags| tags.first())
                .and_then(|tag| {
                    tag.get("label")
                        .or_else(|| tag.get("name"))
                        .and_then(Value::as_str)
                })
                .filter(|name| !name.trim().is_empty())
                .map(ToOwned::to_owned)
        })
}

fn recognize_robinhood_launchpad(
    metadata: &Value,
    creator: Option<&str>,
) -> Option<LaunchpadEvidence> {
    let mut labels = Vec::new();
    collect_label_strings(metadata, &mut labels);
    let joined = labels.join(" ").to_ascii_lowercase();

    const PATTERNS: &[(&[&str], &str)] = &[
        (&["pons"], "Pons"),
        (&["hood.fun", "hoodfun"], "hood.fun"),
        (&["long.xyz", "longxyz"], "Long.xyz"),
        (&["noxa"], "NOXA Fun"),
        (&["coinbarrel"], "Coinbarrel"),
        (&["robinpad"], "Robinpad"),
        (&["stonkbroker"], "StonkBrokers"),
        (&["token.select"], "token.select"),
        (&["hookr"], "hookr.fun"),
        (&["v4.fun", "v4fun"], "v4.fun"),
        (&["raisehood"], "RaiseHood"),
        (&["perpshood"], "PerpsHood"),
        (&["pairyard"], "PairYard"),
        (&["pairex"], "Pairex"),
        (&["unihood"], "Unihood"),
        (&["arrowpad"], "ArrowPad"),
        (&["ponzu"], "Ponzu"),
        (&["merryforge"], "MerryForge"),
        (&["par.family"], "par.family"),
        (&["pyre"], "Pyre"),
        (&["froth"], "Froth"),
        (&["peeps"], "Peeps"),
        (&["pump.fun", "pumpfun"], "Pump.fun"),
    ];

    PATTERNS.iter().find_map(|(patterns, name)| {
        patterns
            .iter()
            .any(|pattern| joined.contains(pattern))
            .then(|| LaunchpadEvidence {
                name: (*name).to_string(),
                family: "Robinhood Chain launchpad".to_string(),
                evidence: match creator {
                    Some(address) => format!(
                        "Blockscout labels creator/factory {address} with metadata matching {name}."
                    ),
                    None => format!("Blockscout creator metadata matches {name}."),
                },
                source: "Blockscout creator/factory label".to_string(),
            })
    })
}

fn collect_label_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if matches!(
                    key.as_str(),
                    "name" | "label" | "display_name" | "implementation_name"
                ) {
                    if let Some(text) = child.as_str() {
                        if !text.trim().is_empty() {
                            output.push(text.to_string());
                        }
                    }
                }
                if key == "public_tags" || key == "implementations" {
                    collect_label_strings(child, output);
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_label_strings(child, output);
            }
        }
        _ => {}
    }
}

async fn erc20_balance_percentage(
    http: &Client,
    rpc_url: &str,
    token: &str,
    wallet: &str,
) -> Result<f64, String> {
    let padded = format!("{:0>64}", wallet.trim_start_matches("0x"));
    let balance_call = format!("0x70a08231{padded}");

    let (balance, supply) = tokio::join!(
        evm_rpc(
            http,
            rpc_url,
            "eth_call",
            json!([{"to": token, "data": balance_call}, "latest"]),
        ),
        evm_rpc(
            http,
            rpc_url,
            "eth_call",
            json!([{"to": token, "data": "0x18160ddd"}, "latest"]),
        )
    );

    let balance = balance?
        .as_str()
        .and_then(hex_biguint)
        .ok_or_else(|| "balanceOf returned invalid data.".to_string())?;
    let supply = supply?
        .as_str()
        .and_then(hex_biguint)
        .ok_or_else(|| "totalSupply returned invalid data.".to_string())?;

    if supply == BigUint::from(0u8) {
        return Err("Token totalSupply is zero.".to_string());
    }

    let balance = balance
        .to_string()
        .parse::<f64>()
        .map_err(|error| error.to_string())?;
    let supply = supply
        .to_string()
        .parse::<f64>()
        .map_err(|error| error.to_string())?;

    Ok((balance / supply * 100.0).clamp(0.0, 100.0))
}

async fn solana_rpc_with_fallback(
    http: &Client,
    primary: &str,
    fallback: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    match solana_rpc(http, primary, method, params.clone()).await {
        Ok(value) => Ok(value),
        Err(primary_error) if fallback != primary => solana_rpc(http, fallback, method, params)
            .await
            .map_err(|fallback_error| {
                format!(
                    "primary Solana RPC failed ({primary_error}); fallback failed ({fallback_error})"
                )
            }),
        Err(error) => Err(error),
    }
}

async fn solana_rpc(
    http: &Client,
    rpc_url: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    json_rpc(http, rpc_url, method, params, "Solana").await
}

async fn evm_rpc(
    http: &Client,
    rpc_url: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    json_rpc(http, rpc_url, method, params, "Robinhood").await
}

async fn json_rpc(
    http: &Client,
    rpc_url: &str,
    method: &str,
    params: Value,
    label: &str,
) -> Result<Value, String> {
    let response = http
        .post(rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        }))
        .send()
        .await
        .map_err(|error| format!("{label} RPC request failed: {error}"))?;

    let status = response.status();
    let body = response
        .json::<Value>()
        .await
        .map_err(|error| format!("{label} RPC returned unreadable JSON: {error}"))?;

    if !status.is_success() {
        return Err(format!("{label} RPC returned HTTP {}.", status.as_u16()));
    }
    if let Some(error) = body.get("error") {
        return Err(format!("{label} RPC error: {error}"));
    }

    body.get("result")
        .cloned()
        .ok_or_else(|| format!("{label} RPC response had no result."))
}

fn hex_biguint(value: &str) -> Option<BigUint> {
    BigUint::parse_bytes(value.trim_start_matches("0x").as_bytes(), 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recognizes_robinhood_launchpad_from_creator_label() {
        let metadata = json!({
            "name": "PonsTokenFactory",
            "is_contract": true
        });

        let launchpad = recognize_robinhood_launchpad(
            &metadata,
            Some("0x1111111111111111111111111111111111111111"),
        )
        .unwrap();

        assert_eq!(launchpad.name, "Pons");
    }

    #[test]
    fn collects_solana_program_ids_from_parsed_transaction() {
        let transaction = json!({
            "transaction": {
                "message": {
                    "accountKeys": [
                        {"pubkey": "11111111111111111111111111111111"}
                    ],
                    "instructions": [
                        {"programId": "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"}
                    ]
                }
            },
            "meta": {
                "innerInstructions": []
            }
        });

        let programs = solana_transaction_program_ids(&transaction);
        assert!(programs.iter().any(|value| value == "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"));
    }

    #[test]
    fn pads_evm_wallet_for_balance_of() {
        let wallet = "1111111111111111111111111111111111111111";
        assert_eq!(format!("{wallet:0>64}").len(), 64);
    }
}
