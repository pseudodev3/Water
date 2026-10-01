use crate::{
    history::robinhood::RobinhoodHistoryClient,
    model::{ChainEvidence, HolderEvidence},
};
use num_bigint::BigUint;
use serde_json::{json, Value};
use tokio::time::{sleep, timeout, Duration};

pub async fn observe(
    http: &reqwest::Client,
    rpc_url: &str,
    holder_index_url: &str,
    holder_index_key: Option<&str>,
    address: &str,
) -> (ChainEvidence, HolderEvidence) {
    let chain_id_request = rpc(http, rpc_url, "eth_chainId", json!([]));
    let code_request = rpc(http, rpc_url, "eth_getCode", json!([address, "latest"]));
    let holder_client = match holder_index_key {
        Some(key) => RobinhoodHistoryClient::with_holder_index(
            http.clone(),
            rpc_url.to_string(),
            holder_index_url.to_string(),
            key.to_string(),
        ),
        None => RobinhoodHistoryClient::new(http.clone(), rpc_url.to_string()),
    };
    let holder_request = async {
        match timeout(
            Duration::from_secs(9),
            holder_client.top_wallet_holders(address, 10),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(
                "Robinhood indexed wallet-holder lookup exceeded the 9s scan budget; market, supply, and chain evidence were returned without waiting longer."
                    .to_string(),
            ),
        }
    };
    let supply_request = erc20_total_supply(http, rpc_url, address);

    let (chain_id_result, code_result, holder_result, supply_result) =
        tokio::join!(chain_id_request, code_request, holder_request, supply_request);

    let chain_id = chain_id_result
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .and_then(|value| u64::from_str_radix(value.trim_start_matches("0x"), 16).ok());

    let verified = code_result
        .as_ref()
        .ok()
        .and_then(Value::as_str)
        .map(|code| code != "0x" && code != "0x0")
        .unwrap_or(false);

    let chain_evidence = ChainEvidence {
        source: "Robinhood Chain JSON-RPC".to_string(),
        verified,
        chain_id,
        detail: if verified {
            format!(
                "Contract bytecode exists on the configured Robinhood RPC{}.",
                chain_id
                    .map(|id| format!(" (chain ID {id})"))
                    .unwrap_or_default()
            )
        } else {
            "The configured Robinhood RPC did not verify contract bytecode.".to_string()
        },
    };

    let total_supply = supply_result.as_ref().ok().copied();

    let concentration = match (total_supply, holder_result.as_ref()) {
        (Some(total), Ok(set)) if total > 0.0 && set.complete_for_requested => {
            let top = set
                .holders
                .iter()
                .fold(rust_decimal::Decimal::ZERO, |sum, holder| {
                    sum + holder.current_quantity
                });

            top.to_string()
                .parse::<f64>()
                .ok()
                .map(|top| (top / total * 100.0).clamp(0.0, 100.0))
        }
        _ => None,
    };

    let holder_detail = match &holder_result {
        Ok(set) if set.complete_for_requested => {
            let excluded = set
                .excluded_contract_quantity
                .to_string()
                .parse::<f64>()
                .ok()
                .map(format_compact)
                .unwrap_or_else(|| "an unrepresentable amount".to_string());

            match concentration {
                Some(value) => format!(
                    "Top indexed wallet addresses hold about {value:.1}% of total supply. Water excluded {} Blockscout-classified contract address{} ({excluded} tokens), so indexed DEX pools, vaults, and protocol contracts are not counted as top wallets.",
                    set.excluded_contracts,
                    if set.excluded_contracts == 1 { "" } else { "es" },
                ),
                None => format!(
                    "Wallet/contract filtering completed, but concentration could not be normalized. {} contract-controlled addresses were excluded.",
                    set.excluded_contracts
                ),
            }
        }
        Ok(set) => format!(
            "Water found {} wallet holder{} after excluding {} contract-controlled address{}, but the classification safety cap was reached before a full top-ten wallet ranking could be proven.",
            set.holders.len(),
            if set.holders.len() == 1 { "" } else { "s" },
            set.excluded_contracts,
            if set.excluded_contracts == 1 { "" } else { "es" },
        ),
        Err(error) => format!(
            "Robinhood wallet-holder reconstruction failed: {error}"
        ),
    };

    let supply_detail = match supply_result {
        Ok(value) => format!(
            "Current ERC-20 total supply is {}.",
            format_compact(value)
        ),
        Err(error) => format!("ERC-20 totalSupply could not be read: {error}"),
    };

    (
        chain_evidence,
        HolderEvidence {
            top_ten_percentage: concentration,
            total_supply,
            source: "Robinhood indexed wallet-holder reconstruction".to_string(),
            detail: format!("{} {}", supply_detail, holder_detail),
        },
    )
}


async fn erc20_total_supply(
    http: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> Result<f64, String> {
    let total_supply_call = rpc(
        http,
        rpc_url,
        "eth_call",
        json!([{"to": address, "data": "0x18160ddd"}, "latest"]),
    );
    let decimals_call = rpc(
        http,
        rpc_url,
        "eth_call",
        json!([{"to": address, "data": "0x313ce567"}, "latest"]),
    );

    let (raw_supply, raw_decimals) = tokio::join!(total_supply_call, decimals_call);
    let raw_supply = raw_supply?
        .as_str()
        .and_then(hex_biguint)
        .ok_or_else(|| "totalSupply returned an invalid uint256.".to_string())?;
    let decimals = raw_decimals?
        .as_str()
        .and_then(hex_biguint)
        .and_then(|value| value.to_string().parse::<u32>().ok())
        .ok_or_else(|| "decimals returned an invalid uint256.".to_string())?;

    scaled_biguint_to_f64(&raw_supply, decimals)
        .ok_or_else(|| "totalSupply could not be normalized to a finite number.".to_string())
}

fn hex_biguint(value: &str) -> Option<BigUint> {
    BigUint::parse_bytes(value.trim_start_matches("0x").as_bytes(), 16)
}

fn scaled_biguint_to_f64(value: &BigUint, decimals: u32) -> Option<f64> {
    let raw = value.to_string();
    if decimals > 255 { return None; }
    let decimals = decimals as usize;
    let normalized = if decimals == 0 {
        raw
    } else if raw.len() <= decimals {
        format!("0.{}{}", "0".repeat(decimals - raw.len()), raw)
    } else {
        let split = raw.len() - decimals;
        format!("{}.{}", &raw[..split], &raw[split..])
    };

    normalized.parse::<f64>().ok().filter(|value| value.is_finite())
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
    timeout(Duration::from_secs(8), rpc_with_retries(http, rpc_url, method, params))
        .await
        .unwrap_or_else(|_| Err(format!("{method} exceeded the 8s scan budget")))
}

async fn rpc_with_retries(
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

    for attempt in 0..3 {
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

    Err(last_error.unwrap_or_else(|| "Robinhood RPC request failed.".to_string()))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_large_erc20_supply() {
        let raw = BigUint::parse_bytes(b"1000000000000000000000000000", 10).unwrap();
        assert_eq!(scaled_biguint_to_f64(&raw, 18), Some(1_000_000_000.0));
    }
}
