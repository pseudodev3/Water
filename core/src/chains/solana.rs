use crate::model::ChainEvidence;
use serde_json::{json, Value};

pub async fn verify(
    http: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> ChainEvidence {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getAccountInfo",
        "params": [address, {"encoding": "base64"}]
    });

    match http.post(rpc_url).json(&payload).send().await {
        Ok(response) => match response.json::<Value>().await {
            Ok(body) => {
                let exists = body
                    .pointer("/result/value")
                    .map(|value| !value.is_null())
                    .unwrap_or(false);

                ChainEvidence {
                    source: "Solana JSON-RPC".to_string(),
                    verified: exists,
                    chain_id: None,
                    detail: if exists {
                        "Token account exists on the configured Solana RPC.".to_string()
                    } else {
                        "No account was returned for this address by the configured Solana RPC.".to_string()
                    },
                }
            }
            Err(error) => failed(error.to_string()),
        },
        Err(error) => failed(error.to_string()),
    }
}

fn failed(detail: String) -> ChainEvidence {
    ChainEvidence {
        source: "Solana JSON-RPC".to_string(),
        verified: false,
        chain_id: None,
        detail: format!("Direct verification failed: {detail}"),
    }
}
