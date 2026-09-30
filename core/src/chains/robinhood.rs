use crate::model::ChainEvidence;
use serde_json::{json, Value};

pub async fn verify(
    http: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> ChainEvidence {
    let chain_id_request = rpc(http, rpc_url, "eth_chainId", json!([]));
    let code_request = rpc(http, rpc_url, "eth_getCode", json!([address, "latest"]));
    let (chain_id_result, code_result) = tokio::join!(chain_id_request, code_request);

    let chain_id = chain_id_result
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .and_then(|value| u64::from_str_radix(value.trim_start_matches("0x"), 16).ok());

    match code_result {
        Ok(value) => {
            let code = value.as_str().unwrap_or("0x");
            let exists = code != "0x" && code != "0x0";

            ChainEvidence {
                source: "Robinhood Chain JSON-RPC".to_string(),
                verified: exists,
                chain_id,
                detail: if exists {
                    format!(
                        "Contract bytecode exists on the configured Robinhood RPC{}.",
                        chain_id.map(|id| format!(" (chain ID {id})")).unwrap_or_default()
                    )
                } else {
                    "No contract bytecode was returned for this address.".to_string()
                },
            }
        }
        Err(error) => ChainEvidence {
            source: "Robinhood Chain JSON-RPC".to_string(),
            verified: false,
            chain_id,
            detail: format!("Direct verification failed: {error}"),
        },
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
