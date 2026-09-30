use crate::model::{ChainEvidence, HolderEvidence};
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};

pub async fn observe(
    http: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> (ChainEvidence, HolderEvidence) {
    // Public Solana RPCs throttle bursts aggressively. Keep these calls sequential
    // and let rpc() back off on 429s instead of firing three requests at once.
    let account = rpc(
        http,
        rpc_url,
        "getAccountInfo",
        json!([address, {"encoding": "base64"}]),
    )
    .await;

    let supply = rpc(http, rpc_url, "getTokenSupply", json!([address])).await;
    sleep(Duration::from_millis(120)).await;
    let largest = rpc(http, rpc_url, "getTokenLargestAccounts", json!([address])).await;

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
            Ok(_) if verified => "Mint account exists on the configured Solana RPC.".to_string(),
            Ok(_) => "The configured Solana RPC returned no mint account.".to_string(),
            Err(error) => format!("Mint verification failed: {error}"),
        },
    };

    let total_supply = supply.as_ref().ok().and_then(normalized_supply);

    let concentration = match (total_supply, largest.as_ref()) {
        (Some(total), Ok(largest)) if total > 0.0 => {
            let top_ten = largest
                .get("value")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .take(10)
                        .filter_map(|row| {
                            row.get("uiAmountString")
                                .and_then(Value::as_str)
                                .and_then(|value| value.parse::<f64>().ok())
                                .or_else(|| {
                                    row.get("uiAmount")
                                        .and_then(Value::as_f64)
                                })
                        })
                        .sum::<f64>()
                });

            top_ten.map(|top_ten| (top_ten / total * 100.0).clamp(0.0, 100.0))
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
            Err(error) => format!("getTokenSupply failed: {error}"),
            Ok(_) => "getTokenSupply returned an unreadable supply value.".to_string(),
        }),
    }

    match concentration {
        Some(value) => details.push(format!(
            "Top ten token accounts hold about {value:.1}% of current supply."
        )),
        None => details.push(match &largest {
            Err(error) => format!("getTokenLargestAccounts failed: {error}"),
            Ok(_) => "Largest-account data was not sufficient to calculate concentration.".to_string(),
        }),
    }

    (
        chain_evidence,
        HolderEvidence {
            top_ten_percentage: concentration,
            total_supply,
            source: "Solana getTokenSupply + getTokenLargestAccounts".to_string(),
            detail: details.join(" "),
        },
    )
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
}
