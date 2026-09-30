use crate::model::{ChainEvidence, HolderEvidence};
use serde_json::{json, Value};

pub async fn observe(
    http: &reqwest::Client,
    rpc_url: &str,
    blockscout_url: &str,
    address: &str,
) -> (ChainEvidence, HolderEvidence) {
    let chain_id_request = rpc(http, rpc_url, "eth_chainId", json!([]));
    let code_request = rpc(http, rpc_url, "eth_getCode", json!([address, "latest"]));
    let token_url = format!(
        "{}/api/v2/tokens/{}",
        blockscout_url.trim_end_matches('/'),
        address
    );
    let holders_url = format!(
        "{}/api/v2/tokens/{}/holders",
        blockscout_url.trim_end_matches('/'),
        address
    );
    let token_request = get_json(http, &token_url);
    let holders_request = get_json(http, &holders_url);

    let (chain_id_result, code_result, token_result, holders_result) =
        tokio::join!(chain_id_request, code_request, token_request, holders_request);

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

    let concentration = match (token_result.as_ref(), holders_result.as_ref()) {
        (Ok(token), Ok(holders)) => {
            let total_supply = find_numeric(token, &["total_supply", "totalSupply"]);
            let top_ten = holders
                .get("items")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .take(10)
                        .filter_map(|row| find_numeric(row, &["value", "token_value", "amount"]))
                        .sum::<f64>()
                });

            match (total_supply, top_ten) {
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
        source: "Robinhood Blockscout token holders".to_string(),
        detail: concentration
            .map(|value| format!("Top ten indexed holders own about {value:.1}% of token supply."))
            .unwrap_or_else(|| {
                "Blockscout did not return enough supply/holder data to calculate concentration."
                    .to_string()
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

async fn get_json(http: &reqwest::Client, url: &str) -> Result<Value, String> {
    let response = http
        .get(url)
        .header("User-Agent", "water/0.1")
        .send()
        .await
        .map_err(|error| error.to_string())?;

    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;

    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), body));
    }

    serde_json::from_str(&body).map_err(|error| error.to_string())
}

fn find_numeric(value: &Value, keys: &[&str]) -> Option<f64> {
    match value {
        Value::Object(object) => {
            for key in keys {
                if let Some(number) = object.get(*key).and_then(value_as_f64) {
                    return Some(number);
                }
            }
            object.values().find_map(|child| find_numeric(child, keys))
        }
        Value::Array(rows) => rows.iter().find_map(|child| find_numeric(child, keys)),
        _ => None,
    }
}

fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(value) => value.replace(',', "").parse::<f64>().ok(),
        _ => None,
    }
}
