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
    pub slug: String,
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
        Chain::Bnb => inspect_bnb(&http, config, &token).await,
    }
}

async fn inspect_bnb(http: &Client, config: &Config, token: &str) -> Result<OriginResponse,String> {
    let chain=evm_rpc(http,&config.bnb_rpc_url,"eth_chainId",json!([])).await?;
    if chain.as_str()!=Some("0x38") {return Err("BNB origin rejected an RPC outside chain 56.".into());}
    let owner=evm_rpc(http,&config.bnb_rpc_url,"eth_call",json!([{"to":token,"data":"0x8da5cb5b"},"latest"])).await.ok()
        .and_then(|v|v.as_str().map(str::to_string)).filter(|s|s.len()==66&&s.starts_with("0x")&&s[2..26].bytes().all(|b|b==b'0')&&s[26..].bytes().all(|b|b.is_ascii_hexdigit()))
        .map(|s|format!("0x{}",&s[26..]).to_ascii_lowercase());
    let zero=owner.as_ref().is_some_and(|s|s[2..].bytes().all(|b|b==b'0'));
    let detail=if zero {"owner() returned the zero address. Creator, proxy permissions and other controls remain unverified."} else if owner.is_some() {"The contract reports this address through owner(). This does not establish the creator or exclude other controls."} else {"No standard owner() address was received. Creator, launchpad and additional controls remain unknown."};
    Ok(OriginResponse{chain:Chain::Bnb,token:token.into(),primary_label:"Reported contract owner".into(),primary_address:owner.filter(|_|!zero),secondary_label:None,secondary_address:None,primary_balance_percentage:None,creator_label:None,launchpad:None,active_controls:if zero {vec!["owner() returns the zero address; other controls unknown".into()]} else {vec![]},source:"BNB Chain JSON-RPC owner() read".into(),detail:detail.into()})
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
    let indexed = async {
        tokio::time::timeout(std::time::Duration::from_secs(7), inspect_robinhood_indexed(http, config, token))
            .await.unwrap_or_else(|_| Err("Explorer origin lookup exceeded its 7s budget.".to_string()))
    };
    let (factory_match, indexed) = tokio::join!(
        crate::robinhood_launchpads::detect(http, &config.robinhood_rpc_url, token),
        indexed,
    );
    let mut response = indexed.unwrap_or_else(|error| OriginResponse {
        chain: Chain::Robinhood,
        token: token.to_string(),
        primary_label: "Contract creator".to_string(),
        primary_address: None,
        secondary_label: None,
        secondary_address: None,
        primary_balance_percentage: None,
        creator_label: None,
        launchpad: None,
        active_controls: vec![],
        source: "Robinhood JSON-RPC; explorer evidence unavailable".to_string(),
        detail: format!("{error} Unavailable creator controls remain unknown."),
    });
    if let Some(found) = factory_match {
        // The factory's launch creator may differ from the contract that executed CREATE.
        // Preserve indexed creator/control semantics; use the launch creator only if absent.
        if response.primary_address.is_none() {
            response.primary_label = "Launch creator".to_string();
            response.primary_address = Some(found.creator);
        }
        response.launchpad = Some(found.evidence);
        response.source = "Robinhood verified launch evidence + available indexed origin evidence".to_string();
    }
    Ok(response)
}

async fn inspect_robinhood_indexed(
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
        .timeout(std::time::Duration::from_secs(3))
        .header("Accept", "application/json")
        .header("User-Agent", "water/0.1")
        .query(&[("apikey", key)])
        .send()
        .await
        .map_err(|error| format!("Blockscout origin lookup failed: {}", error.without_url()))?;

    let status = response.status();
    let body = response
        .json::<Value>()
        .await
        .map_err(|error| format!("Blockscout origin lookup returned unreadable JSON: {}", error.without_url()))?;

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
    let creator_metadata_request = async { match creator.as_deref() {
        Some(address) => blockscout_address(http, config, address).await.ok(),
        None => None,
    }};
    let creator_balance_request = async { match creator.as_deref() {
        Some(address) => erc20_balance_percentage(http, &config.robinhood_rpc_url, token, address).await.ok(),
        None => None,
    }};
    let creator_code_request = async { match creator.as_deref() {
        Some(address) => evm_rpc(http, &config.robinhood_rpc_url, "eth_getCode", json!([address,"latest"])).await.ok(),
        None => None,
    }};
    let (creator_metadata, creator_balance_percentage, creator_code) = tokio::join!(creator_metadata_request, creator_balance_request, creator_code_request);
    let creator_label = creator_metadata
        .as_ref()
        .and_then(blockscout_best_label);
    let launchpad = creator_metadata
        .as_ref()
        .and_then(|metadata| recognize_robinhood_launchpad(metadata, creator.as_deref()));

    let mut active_controls = Vec::new();
    if let Some(code) = creator_code.as_ref().and_then(Value::as_str) {
        if code != "0x" && code != "0x0" {
            active_controls.push("Creator address is a contract".to_string());
        } else {
            active_controls.push("Creator address is an EOA wallet".to_string());
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
    const SIGNATURE_PAGE_LIMIT: usize = 1_000;
    const MAX_SIGNATURE_PAGES: usize = 2;
    const OLDEST_CANDIDATES_PER_PAGE: usize = 5;

    let mut before: Option<String> = None;
    let mut candidate_signatures = Vec::new();

    for _ in 0..MAX_SIGNATURE_PAGES {
        let mut options = serde_json::Map::new();
        options.insert("limit".to_string(), json!(SIGNATURE_PAGE_LIMIT));
        if let Some(cursor) = before.as_ref() {
            options.insert("before".to_string(), json!(cursor));
        }

        let signatures = solana_rpc_with_fallback(
            http,
            &config.solana_rpc_url,
            &config.solana_fallback_rpc_url,
            "getSignaturesForAddress",
            json!([mint, Value::Object(options)]),
        )
        .await
        .ok()?;

        let rows = signatures.as_array()?;
        if rows.is_empty() {
            break;
        }

        for row in rows.iter().rev().take(OLDEST_CANDIDATES_PER_PAGE) {
            if let Some(signature) = row.get("signature").and_then(Value::as_str) {
                candidate_signatures.push(signature.to_string());
            }
        }

        before = rows
            .last()
            .and_then(|row| row.get("signature"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        if rows.len() < SIGNATURE_PAGE_LIMIT {
            break;
        }
    }

    // Check the oldest candidates first. Water only names the launchpad when it
    // sees a launch-specific fingerprint; shared Raydium infrastructure alone
    // is deliberately not enough to call something StonkFun.
    candidate_signatures.reverse();
    candidate_signatures.dedup();

    for signature in candidate_signatures {
        let transaction = match solana_rpc_with_fallback(
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
        {
            Ok(transaction) => transaction,
            Err(_) => continue,
        };

        if let Some(launchpad) = recognize_solana_launchpad(&transaction) {
            return Some(launchpad);
        }
    }

    None
}

fn recognize_solana_launchpad(transaction: &Value) -> Option<LaunchpadEvidence> {
    const PUMP_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
    const PUMP_MAYHEM_PROGRAM: &str = "MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e";

    const STONKFUN_STANDARD_CONFIG: &str =
        "4E876qZTE9FJMrBzgVtBrSrzz2TLivB5Y5QXPjB4gZL7";
    const STONKFUN_REWARD_CONFIG: &str =
        "6BwHHDg3u1854jC8PDLXvR4spTcLNaoBxLJNGC4nTESt";
    const STONKFUN_LAUNCHER: &str =
        "5CEbueQnq1Ym2uSSx2xXds3jQAqT1BDnkA59RZobSPAG";
    const RAYDIUM_CLMM_PROGRAM: &str =
        "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";

    let accounts = solana_transaction_account_keys(transaction);
    let programs = solana_transaction_program_ids(transaction);

    if accounts.iter().any(|address| address == STONKFUN_REWARD_CONFIG) {
        return Some(LaunchpadEvidence {
            slug: "stonkfun".to_string(),
            name: "StonkFun".to_string(),
            family: "StonkFun".to_string(),
            evidence: format!(
                "Launch transaction references StonkFun reward config {STONKFUN_REWARD_CONFIG}."
            ),
            source: "Solana launch-config match".to_string(),
        });
    }

    if accounts.iter().any(|address| address == STONKFUN_STANDARD_CONFIG) {
        return Some(LaunchpadEvidence {
            slug: "stonkfun".to_string(),
            name: "StonkFun".to_string(),
            family: "StonkFun".to_string(),
            evidence: format!(
                "Launch transaction references StonkFun standard config {STONKFUN_STANDARD_CONFIG}."
            ),
            source: "Solana launch-config match".to_string(),
        });
    }

    // StonkFun's older direct-pool launches predate its LaunchLab configs. The
    // launcher signer plus the Raydium CLMM program is the conservative legacy
    // fingerprint; CLMM by itself is not treated as StonkFun.
    if accounts.iter().any(|address| address == STONKFUN_LAUNCHER)
        && programs.iter().any(|program| program == RAYDIUM_CLMM_PROGRAM)
    {
        return Some(LaunchpadEvidence {
            slug: "stonkfun".to_string(),
            name: "StonkFun".to_string(),
            family: "StonkFun".to_string(),
            evidence: format!(
                "Legacy launch transaction contains StonkFun launcher {STONKFUN_LAUNCHER} and Raydium CLMM."
            ),
            source: "Solana launcher + program match".to_string(),
        });
    }

    if programs
        .iter()
        .any(|program| program == PUMP_PROGRAM || program == PUMP_MAYHEM_PROGRAM)
    {
        return Some(LaunchpadEvidence {
            slug: "pumpfun".to_string(),
            name: "Pump.fun".to_string(),
            family: "Pump.fun".to_string(),
            evidence: "Launch transaction invokes a Pump.fun launch program.".to_string(),
            source: "Solana launch-program match".to_string(),
        });
    }

    None
}

fn solana_transaction_account_keys(transaction: &Value) -> Vec<String> {
    let mut accounts = transaction
        .pointer("/transaction/message/accountKeys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|key| {
            key.as_str()
                .or_else(|| key.get("pubkey").and_then(Value::as_str))
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>();

    if let Some(loaded) = transaction.pointer("/meta/loadedAddresses") {
        for side in ["writable", "readonly"] {
            if let Some(values) = loaded.get(side).and_then(Value::as_array) {
                accounts.extend(
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned),
                );
            }
        }
    }

    accounts.sort();
    accounts.dedup();
    accounts
}

fn solana_transaction_program_ids(transaction: &Value) -> Vec<String> {
    let mut programs = Vec::new();

    // jsonParsed resolves programId for top-level and inner instructions.
    // Do not treat every account key as an invoked program: launchpad IDs are
    // evidence only when the transaction actually executes them.
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
        .timeout(std::time::Duration::from_secs(3))
        .header("Accept", "application/json")
        .header("User-Agent", "water/0.1")
        .query(&[("apikey", key)])
        .send()
        .await
        .map_err(|error| format!("Blockscout creator metadata failed: {}", error.without_url()))?;

    let status = response.status();
    let body = response
        .json::<Value>()
        .await
        .map_err(|error| format!("Blockscout creator metadata returned unreadable JSON: {}", error.without_url()))?;

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
    let labels: Vec<_> = labels.iter().map(|label| label.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()).collect();

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
            .any(|pattern| {
                let pattern: String = pattern.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
                labels.iter().any(|label| {
                    label == &pattern || ["factory", "tokenfactory", "launchfactory", "launcher", "launchdeployer"]
                        .iter().any(|suffix| label == &format!("{pattern}{suffix}"))
                })
            })
            .then(|| LaunchpadEvidence {
                slug: robinhood_launchpad_slug(name).to_string(),
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

fn robinhood_launchpad_slug(name: &str) -> &'static str {
    match name {
        "Pons" => "pons",
        "hood.fun" => "hoodfun",
        "Long.xyz" => "longxyz",
        "NOXA Fun" => "noxa",
        "Coinbarrel" => "coinbarrel",
        "Robinpad" => "robinpad",
        "StonkBrokers" => "stonkbrokers",
        "token.select" => "tokenselect",
        "hookr.fun" => "hookr",
        "v4.fun" => "v4fun",
        "RaiseHood" => "raisehood",
        "PerpsHood" => "perpshood",
        "PairYard" => "pairyard",
        "Pairex" => "pairex",
        "Unihood" => "unihood",
        "ArrowPad" => "arrowpad",
        "Ponzu" => "ponzu",
        "MerryForge" => "merryforge",
        "par.family" => "parfamily",
        "Pyre" => "pyre",
        "Froth" => "froth",
        "Peeps" => "peeps",
        "Pump.fun" => "pumpfun",
        _ => "unknown",
    }
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
        .timeout(std::time::Duration::from_secs(if label == "Robinhood" { 3 } else { 12 }))
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
    fn unrelated_substrings_do_not_become_launchpad_labels() {
        for name in ["ResponseFactory", "SponsoredToken", "NotPonsTokenFactory", "FrothyToken", "Pons Impersonator"] {
            assert!(recognize_robinhood_launchpad(&json!({"name":name}), None).is_none(), "{name}");
        }
        assert_eq!(recognize_robinhood_launchpad(&json!({"name":"Pons: Token Factory"}), None).unwrap().slug, "pons");
    }

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
    fn recognizes_stonkfun_before_shared_launchlab_infrastructure() {
        let transaction = json!({
            "transaction": {
                "message": {
                    "accountKeys": [
                        {"pubkey": "4E876qZTE9FJMrBzgVtBrSrzz2TLivB5Y5QXPjB4gZL7"},
                        {"pubkey": "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj"}
                    ],
                    "instructions": [
                        {"programId": "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj"}
                    ]
                }
            },
            "meta": {"innerInstructions": []}
        });

        let launchpad = recognize_solana_launchpad(&transaction).unwrap();
        assert_eq!(launchpad.slug, "stonkfun");
        assert_eq!(launchpad.name, "StonkFun");
    }

    #[test]
    fn generic_launchlab_is_not_mislabeled_stonkfun() {
        let transaction = json!({
            "transaction": {
                "message": {
                    "accountKeys": [
                        {"pubkey": "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj"}
                    ],
                    "instructions": [
                        {"programId": "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj"}
                    ]
                }
            },
            "meta": {"innerInstructions": []}
        });

        assert!(recognize_solana_launchpad(&transaction).is_none());
    }

    #[test]
    fn recognizes_pumpfun_program() {
        let transaction = json!({
            "transaction": {
                "message": {
                    "accountKeys": [],
                    "instructions": [
                        {"programId": "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"}
                    ]
                }
            },
            "meta": {"innerInstructions": []}
        });

        let launchpad = recognize_solana_launchpad(&transaction).unwrap();
        assert_eq!(launchpad.slug, "pumpfun");
        assert_eq!(launchpad.name, "Pump.fun");
    }

    #[test]
    fn pads_evm_wallet_for_balance_of() {
        let wallet = "1111111111111111111111111111111111111111";
        assert_eq!(format!("{wallet:0>64}").len(), 64);
    }
}
