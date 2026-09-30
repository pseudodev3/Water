use crate::model::{ChainEvidence, HolderEvidence};
use curve25519_dalek::edwards::CompressedEdwardsY;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";
const FALLBACK_SOLANA_RPC: &str = "https://solana-rpc.publicnode.com";

pub async fn observe(
    http: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> (ChainEvidence, HolderEvidence) {
    let account = rpc_with_fallback(
        http,
        rpc_url,
        "getAccountInfo",
        json!([address, {"encoding": "base64"}]),
    )
    .await;

    let supply = rpc_with_fallback(http, rpc_url, "getTokenSupply", json!([address])).await;
    sleep(Duration::from_millis(120)).await;
    let largest =
        rpc_with_fallback(http, rpc_url, "getTokenLargestAccounts", json!([address])).await;

    let verified = account
        .as_ref()
        .ok()
        .map(|value| !value.is_null())
        .unwrap_or(false);

    let chain_evidence = ChainEvidence {
        source: "Solana JSON-RPC".to_string(),
        verified,
        chain_id: None,
        detail: match &account {
            Ok(_) if verified => "Mint account exists on Solana RPC.".to_string(),
            Ok(_) => "Solana RPC returned no mint account.".to_string(),
            Err(error) => format!("Mint verification failed on primary and fallback RPCs: {error}"),
        },
    };

    let total_supply = supply.as_ref().ok().and_then(normalized_supply);

    let concentration = match (&largest, total_supply) {
        (Ok(largest), Some(total)) if total > 0.0 => {
            wallet_holder_concentration(http, rpc_url, largest, total).await.ok()
        }
        _ => None,
    };

    let mut details = Vec::new();

    match total_supply {
        Some(value) => details.push(format!(
            "Current onchain token supply is {}.",
            format_compact(value)
        )),
        None => details.push(match &supply {
            Err(error) => format!("getTokenSupply failed on primary and fallback RPCs: {error}"),
            Ok(_) => "getTokenSupply returned an unreadable supply value.".to_string(),
        }),
    }

    match &concentration {
        Some(value) => {
            details.push(format!(
                "Top {} wallet-like authorities among the largest token accounts hold about {:.1}% of current supply.",
                value.wallets_used, value.percentage
            ));
            if value.excluded_non_wallet_count > 0 {
                details.push(format!(
                    "Excluded {} program/PDA-controlled authorit{} totaling about {:.1}% of supply from the wallet-holder numerator.",
                    value.excluded_non_wallet_count,
                    if value.excluded_non_wallet_count == 1 { "y" } else { "ies" },
                    value.excluded_non_wallet_percentage
                ));
            }
            if !value.authority_account_classification_complete {
                details.push(
                    "Program-account metadata was partially unavailable, so on-curve authority checks were used conservatively."
                        .to_string(),
                );
            }
        }
        None => details.push(match &largest {
            Err(error) => format!(
                "getTokenLargestAccounts failed on primary and fallback RPCs: {error}"
            ),
            Ok(_) => "Largest-account data was not sufficient to calculate wallet-holder concentration."
                .to_string(),
        }),
    }

    (
        chain_evidence,
        HolderEvidence {
            top_ten_percentage: concentration.as_ref().map(|value| value.percentage),
            total_supply,
            source: "Solana wallet-holder reconstruction".to_string(),
            detail: details.join(" "),
        },
    )
}

#[derive(Debug)]
struct WalletConcentration {
    percentage: f64,
    wallets_used: usize,
    excluded_non_wallet_count: usize,
    excluded_non_wallet_percentage: f64,
    authority_account_classification_complete: bool,
}

async fn wallet_holder_concentration(
    http: &reqwest::Client,
    rpc_url: &str,
    largest: &Value,
    total_supply: f64,
) -> Result<WalletConcentration, String> {
    let rows = largest
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "getTokenLargestAccounts returned no value array.".to_string())?;

    let token_accounts: Vec<String> = rows
        .iter()
        .filter_map(|row| row.get("address").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect();

    if token_accounts.is_empty() {
        return Err("No largest token accounts were returned.".to_string());
    }

    let infos = rpc_with_fallback(
        http,
        rpc_url,
        "getMultipleAccounts",
        json!([
            token_accounts,
            {"encoding": "jsonParsed", "commitment": "confirmed"}
        ]),
    )
    .await?;

    let info_rows = infos
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "Token-account metadata was unavailable.".to_string())?;

    let mut by_authority: HashMap<String, f64> = HashMap::new();

    for (row, info) in rows.iter().zip(info_rows.iter()) {
        let Some(authority) = info
            .pointer("/data/parsed/info/owner")
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(quantity) = row_quantity(row) else {
            continue;
        };

        *by_authority.entry(authority.to_string()).or_insert(0.0) += quantity;
    }

    if by_authority.is_empty() {
        return Err("No token-account authorities could be resolved.".to_string());
    }

    let authorities: Vec<String> = by_authority.keys().cloned().collect();
    let authority_infos = rpc_with_fallback(
        http,
        rpc_url,
        "getMultipleAccounts",
        json!([
            authorities,
            {"encoding": "base64", "commitment": "confirmed"}
        ]),
    )
    .await;

    let authority_values = authority_infos
        .as_ref()
        .ok()
        .and_then(|value| value.get("value"))
        .and_then(Value::as_array);

    let classification_complete = authority_values.is_some();

    let mut wallet_balances = Vec::new();
    let mut excluded_balance = 0.0;
    let mut excluded_count = 0usize;

    for (index, (authority, balance)) in by_authority.into_iter().enumerate() {
        let on_curve = is_on_curve(&authority);
        let account_value = authority_values.and_then(|values| values.get(index));

        let wallet_like = if !on_curve {
            false
        } else {
            match account_value {
                Some(Value::Null) | None => true,
                Some(value) => {
                    let owner = value.get("owner").and_then(Value::as_str);
                    let executable = value
                        .get("executable")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    owner == Some(SYSTEM_PROGRAM) && !executable
                }
            }
        };

        if wallet_like {
            wallet_balances.push(balance);
        } else {
            excluded_balance += balance;
            excluded_count += 1;
        }
    }

    wallet_balances.sort_by(|left, right| {
        right
            .partial_cmp(left)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // getTokenLargestAccounts only returns 20 token accounts. We therefore take
    // up to ten distinct wallet-like authorities from that ranked window.
    let wallets_used = wallet_balances.len().min(10);
    if wallets_used == 0 {
        return Err("No wallet-like authorities were found in the largest-account window.".to_string());
    }

    let top_wallet_balance: f64 = wallet_balances.iter().take(10).sum();

    Ok(WalletConcentration {
        percentage: (top_wallet_balance / total_supply * 100.0).clamp(0.0, 100.0),
        wallets_used,
        excluded_non_wallet_count: excluded_count,
        excluded_non_wallet_percentage: (excluded_balance / total_supply * 100.0).clamp(0.0, 100.0),
        authority_account_classification_complete: classification_complete,
    })
}

fn is_on_curve(address: &str) -> bool {
    let Ok(bytes) = bs58::decode(address).into_vec() else {
        return false;
    };
    let Ok(bytes): Result<[u8; 32], _> = bytes.try_into() else {
        return false;
    };

    CompressedEdwardsY(bytes).decompress().is_some()
}

fn row_quantity(row: &Value) -> Option<f64> {
    row.get("uiAmountString")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .or_else(|| row.get("uiAmount").and_then(Value::as_f64))
        .or_else(|| {
            let raw = row.get("amount")?.as_str()?.parse::<f64>().ok()?;
            let decimals = row.get("decimals")?.as_u64()? as i32;
            Some(raw / 10f64.powi(decimals))
        })
}

fn normalized_supply(value: &Value) -> Option<f64> {
    value
        .pointer("/value/uiAmountString")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .or_else(|| value.pointer("/value/uiAmount").and_then(Value::as_f64))
        .or_else(|| {
            let raw = value
                .pointer("/value/amount")
                .and_then(Value::as_str)?
                .parse::<f64>()
                .ok()?;
            let decimals = value.pointer("/value/decimals")?.as_u64()? as i32;
            Some(raw / 10f64.powi(decimals))
        })
}

fn format_compact(value: f64) -> String {
    if value >= 1_000_000_000.0 {
        format!("{:.2}B", value / 1_000_000_000.0)
    } else if value >= 1_000_000.0 {
        format!("{:.2}M", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("{:.2}K", value / 1_000.0)
    } else {
        format!("{value:.4}")
    }
}

async fn rpc_with_fallback(
    http: &reqwest::Client,
    primary_rpc_url: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    match rpc(http, primary_rpc_url, method, params.clone()).await {
        Ok(value) => Ok(value),
        Err(primary_error) => {
            if primary_rpc_url.trim_end_matches('/') == FALLBACK_SOLANA_RPC {
                return Err(primary_error);
            }

            rpc(http, FALLBACK_SOLANA_RPC, method, params)
                .await
                .map_err(|fallback_error| {
                    format!("primary: {primary_error}; fallback: {fallback_error}")
                })
        }
    }
}

async fn rpc(
    http: &reqwest::Client,
    rpc_url: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    });

    let mut last_error = None;

    for attempt in 0..4 {
        match http.post(rpc_url).json(&payload).send().await {
            Ok(response) => {
                let status = response.status();
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(1 + attempt as u64);
                let body = response
                    .json::<Value>()
                    .await
                    .map_err(|error| error.to_string())?;

                if status.as_u16() == 429 {
                    last_error = Some(format!("HTTP 429: {body}"));
                    sleep(Duration::from_secs(retry_after.min(5))).await;
                    continue;
                }

                if !status.is_success() {
                    return Err(format!("HTTP {}: {body}", status.as_u16()));
                }

                if let Some(error) = body.get("error") {
                    let code = error.get("code").and_then(Value::as_i64);
                    if code == Some(429) {
                        last_error = Some(error.to_string());
                        sleep(Duration::from_secs((1 + attempt as u64).min(5))).await;
                        continue;
                    }
                    return Err(error.to_string());
                }

                return body
                    .get("result")
                    .cloned()
                    .ok_or_else(|| "RPC response did not contain a result.".to_string());
            }
            Err(error) => {
                last_error = Some(error.to_string());
                sleep(Duration::from_millis(400 * (attempt + 1) as u64)).await;
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        format!("{method} failed after bounded retries.")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_ui_supply_first() {
        let value = json!({
            "value": {
                "amount": "999999000000",
                "decimals": 6,
                "uiAmountString": "999999"
            }
        });

        assert_eq!(normalized_supply(&value), Some(999_999.0));
    }

    #[test]
    fn falls_back_to_raw_supply_and_decimals() {
        let value = json!({
            "value": {
                "amount": "42000000",
                "decimals": 6
            }
        });

        assert_eq!(normalized_supply(&value), Some(42.0));
    }

    #[test]
    fn system_program_is_on_curve() {
        assert!(is_on_curve(SYSTEM_PROGRAM));
    }

    #[test]
    fn invalid_address_is_not_on_curve() {
        assert!(!is_on_curve("not-a-solana-address"));
    }
}
