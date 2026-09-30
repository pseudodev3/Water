use crate::{
    history::robinhood::RobinhoodHistoryClient,
    model::{ChainEvidence, HolderEvidence},
};
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};

pub async fn observe(
    http: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> (ChainEvidence, HolderEvidence) {
    let chain_id_request = rpc(http, rpc_url, "eth_chainId", json!([]));
    let code_request = rpc(http, rpc_url, "eth_getCode", json!([address, "latest"]));
    let holder_client = RobinhoodHistoryClient::new(http.clone(), rpc_url.to_string());
    let concentration_request = holder_client.top_holder_concentration(address, 10);

    let (chain_id_result, code_result, concentration_result) =
        tokio::join!(chain_id_request, code_request, concentration_request);

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

    let (concentration, detail) = match concentration_result {
        Ok(value) => (
            Some(value),
            format!(
                "Top ten positive holder balances reconstructed from ERC-20 Transfer logs own about {value:.1}% of replayed circulating balances."
            ),
        ),
        Err(error) => (
            None,
            format!(
                "Robinhood holder concentration could not be reconstructed from public RPC logs: {error}"
            ),
        ),
    };

    (
        chain_evidence,
        HolderEvidence {
            top_ten_percentage: concentration,
            source: "Robinhood JSON-RPC ERC-20 Transfer replay".to_string(),
            detail,
        },
    )
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
