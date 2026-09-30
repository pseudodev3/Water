use crate::{
    history::solana::SolanaHistoryClient,
    model::{ChainEvidence, HolderEvidence},
};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};

pub async fn observe(
    http: &reqwest::Client,
    rpc_url: &str,
    fallback_rpc_url: &str,
    address: &str,
) -> (ChainEvidence, HolderEvidence) {
    let account = rpc_with_fallback(
        http,
        rpc_url,
        fallback_rpc_url,
        "getAccountInfo",
        json!([address, {"encoding": "base64"}]),
    )
    .await;

    let supply = rpc_with_fallback(
        http,
        rpc_url,
        fallback_rpc_url,
        "getTokenSupply",
        json!([address]),
    )
    .await;

    let holder_client = SolanaHistoryClient::with_fallback(
        http.clone(),
        rpc_url.to_string(),
        fallback_rpc_url.to_string(),
    );
    let holder_set = holder_client.top_wallet_holders(address, 10).await;

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

    let concentration = match (total_supply, holder_set.as_ref()) {
        (Some(total), Ok(set)) if total > 0.0 && set.complete_for_requested => {
            let top = set
                .holders
                .iter()
                .fold(Decimal::ZERO, |sum, holder| sum + holder.current_quantity);
            top.to_string()
                .parse::<f64>()
                .ok()
                .map(|top| (top / total * 100.0).clamp(0.0, 100.0))
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

    match &holder_set {
        Ok(set) if set.complete_for_requested => {
            let excluded = decimal_to_f64(set.excluded_program_quantity)
                .map(format_compact)
                .unwrap_or_else(|| "an unrepresentable amount".to_string());

            match concentration {
                Some(value) => details.push(format!(
                    "Top wallet authorities hold about {value:.1}% of total supply. Water grouped token accounts by authority and excluded {} program/PDA-controlled authorit{} ({excluded} tokens) from the {} largest token accounts.",
                    set.excluded_program_authorities,
                    if set.excluded_program_authorities == 1 { "y" } else { "ies" },
                    set.scanned_token_accounts
                )),
                None => details.push(format!(
                    "Wallet-authority filtering completed, but concentration could not be normalized. {} program/PDA-controlled authorities were excluded.",
                    set.excluded_program_authorities
                )),
            }
        }
        Ok(set) => details.push(format!(
            "Water excluded {} program/PDA-controlled authorit{} from the {} largest token accounts, but only {} wallet authorit{} remained. Standard getTokenLargestAccounts only exposes 20 token accounts, so a true top-ten wallet ranking cannot be proven without an indexer.",
            set.excluded_program_authorities,
            if set.excluded_program_authorities == 1 { "y" } else { "ies" },
            set.scanned_token_accounts,
            set.holders.len(),
            if set.holders.len() == 1 { "y" } else { "ies" },
        )),
        Err(error) => details.push(format!(
            "Wallet-holder reconstruction failed: {error}"
        )),
    }

    (
        chain_evidence,
        HolderEvidence {
            top_ten_percentage: concentration,
            total_supply,
            source: "Solana wallet-authority holder reconstruction".to_string(),
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

fn decimal_to_f64(value: Decimal) -> Option<f64> {
    value.to_string().parse::<f64>().ok()
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
    rpc_url: &str,
    fallback_rpc_url: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    match rpc(http, rpc_url, method, params.clone()).await {
        Ok(value) => Ok(value),
        Err(primary_error) if fallback_rpc_url != rpc_url => {
            rpc(http, fallback_rpc_url, method, params)
                .await
                .map_err(|fallback_error| {
                    format!(
                        "primary RPC failed ({primary_error}); fallback RPC failed ({fallback_error})"
                    )
                })
        }
        Err(error) => Err(error),
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
