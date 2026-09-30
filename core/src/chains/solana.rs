use crate::model::{ChainEvidence, HolderEvidence};
use serde_json::{json, Value};

pub async fn observe(
    http: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> (ChainEvidence, HolderEvidence) {
    let account = rpc(
        http,
        rpc_url,
        "getAccountInfo",
        json!([address, {"encoding": "base64"}]),
    );
    let supply = rpc(http, rpc_url, "getTokenSupply", json!([address]));
    let largest = rpc(http, rpc_url, "getTokenLargestAccounts", json!([address]));

    let (account, supply, largest) = tokio::join!(account, supply, largest);

    let verified = account
        .as_ref()
        .ok()
        .map(|value| !value.is_null())
        .unwrap_or(false);

    let chain_evidence = ChainEvidence {
        source: "Solana JSON-RPC".to_string(),
        verified,
        chain_id: None,
        detail: if verified {
            "Mint account exists on the configured Solana RPC.".to_string()
        } else {
            "The configured Solana RPC did not verify this mint account.".to_string()
        },
    };

    let concentration = match (supply.as_ref(), largest.as_ref()) {
        (Ok(supply), Ok(largest)) => {
            let total = supply
                .pointer("/value/amount")
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<f64>().ok());

            let top_ten = largest
                .get("value")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .take(10)
                        .filter_map(|row| {
                            row.get("amount")
                                .and_then(Value::as_str)
                                .and_then(|value| value.parse::<f64>().ok())
                        })
                        .sum::<f64>()
                });

            match (total, top_ten) {
                (Some(total), Some(top_ten)) if total > 0.0 => {
                    Some((top_ten / total * 100.0).clamp(0.0, 100.0))
                }
                _ => None,
            }
        }
        _ => None,
    };

    let holder_evidence = HolderEvidence {
        top_ten_percentage: concentration,
        source: "Solana getTokenSupply + getTokenLargestAccounts".to_string(),
        detail: concentration
            .map(|value| format!("Top ten token accounts hold about {value:.1}% of current supply."))
            .unwrap_or_else(|| {
                "Holder concentration was unavailable from the configured Solana RPC.".to_string()
            }),
    };

    (chain_evidence, holder_evidence)
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

    let body = http
        .post(rpc_url)
        .json(&payload)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .json::<Value>()
        .await
        .map_err(|error| error.to_string())?;

    if let Some(error) = body.get("error") {
        return Err(error.to_string());
    }

    body.get("result")
        .cloned()
        .ok_or_else(|| "RPC response did not contain a result.".to_string())
}
