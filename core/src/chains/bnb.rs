use super::robinhood::{erc20_total_supply, format_compact, rpc};
use crate::model::{ChainEvidence, HolderEvidence};
use serde_json::{json, Value};

pub async fn observe(
    http: &reqwest::Client,
    url: &str,
    fallback: &str,
    token: &str,
) -> (ChainEvidence, HolderEvidence) {
    let (chain, code, supply) = tokio::join!(
        read(http, url, fallback, "eth_chainId", json!([])),
        read(http, url, fallback, "eth_getCode", json!([token, "latest"])),
        supply(http, url, fallback, token)
    );
    let chain_id = chain.ok().and_then(|v| {
        v.as_str()
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
    });
    let correct = chain_id == Some(56);
    let verified = correct
        && code
            .ok()
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|s| s != "0x" && s != "0x0");
    let total_supply = if correct {
        supply.as_ref().ok().copied()
    } else {
        None
    };
    let detail = match total_supply {
        Some(n)=>format!("Current ERC-20 total supply is {}. A complete wallet-holder index is not configured; concentration remains unknown.",format_compact(n)),
        None=>"BNB supply could not be verified on chain 56; wallet-holder concentration remains unknown.".into(),
    };
    (
        ChainEvidence {
            source: "BNB Chain JSON-RPC".into(),
            verified,
            chain_id,
            detail: if verified {
                "Contract bytecode verified on BNB Smart Chain (chain ID 56).".into()
            } else {
                "The configured RPC did not verify this contract on BNB Smart Chain (chain ID 56)."
                    .into()
            },
        },
        HolderEvidence {
            top_ten_percentage: None,
            total_supply,
            source: "BNB ERC-20 supply; wallet-holder index unavailable".into(),
            detail,
        },
    )
}

async fn read(
    http: &reqwest::Client,
    primary: &str,
    fallback: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    match rpc(http, primary, method, params.clone()).await {
        Ok(value) => Ok(value),
        Err(_) => {
            if rpc(http, fallback, "eth_chainId", json!([]))
                .await?
                .as_str()
                != Some("0x38")
            {
                return Err("BNB fallback rejected an RPC outside chain 56.".into());
            }
            rpc(http, fallback, method, params).await
        }
    }
}

async fn supply(
    http: &reqwest::Client,
    primary: &str,
    fallback: &str,
    token: &str,
) -> Result<f64, String> {
    match erc20_total_supply(http, primary, token).await {
        Ok(value) => Ok(value),
        Err(_) => {
            if rpc(http, fallback, "eth_chainId", json!([]))
                .await?
                .as_str()
                != Some("0x38")
            {
                return Err("BNB fallback rejected an RPC outside chain 56.".into());
            }
            erc20_total_supply(http, fallback, token).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    #[tokio::test]
    async fn a_wrong_chain_cannot_verify_bytecode_or_publish_supply() {
        async fn rpc_stub(Json(v): Json<Value>) -> Json<Value> {
            Json(
                json!({"jsonrpc":"2.0","id":1,"result":match v["method"].as_str().unwrap(){"eth_chainId"=>"0x1237","eth_getCode"=>"0x1234",_=>"0x12"}}),
            )
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/", post(rpc_stub)))
                .await
                .unwrap()
        });
        let (chain, holders) = observe(
            &reqwest::Client::new(),
            &url,
            &url,
            "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c",
        )
        .await;
        assert_eq!(chain.chain_id, Some(4663));
        assert!(!chain.verified);
        assert!(holders.total_supply.is_none() && holders.top_ten_percentage.is_none());
        task.abort();
    }
}
