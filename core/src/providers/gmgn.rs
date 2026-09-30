use crate::model::Chain;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::time::sleep;
use uuid::Uuid;

#[derive(Clone)]
pub struct GmgnClient {
    http: reqwest::Client,
    api_key: Option<String>,
    host: String,
}

#[derive(Debug, Error)]
pub enum GmgnError {
    #[error("GMGN_API_KEY is not configured")]
    NotConfigured,
    #[error("GMGN request failed: {0}")]
    Transport(String),
    #[error("GMGN returned HTTP {0}: {1}")]
    Http(u16, String),
    #[error("GMGN API error: {0}")]
    Api(String),
    #[error("GMGN returned an unreadable response")]
    InvalidResponse,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    code: Value,
    data: Option<Value>,
    message: Option<String>,
    error: Option<String>,
}

impl GmgnClient {
    pub fn new(http: reqwest::Client, api_key: Option<String>, host: String) -> Self {
        Self {
            http,
            api_key,
            host: host.trim_end_matches('/').to_string(),
        }
    }

    pub fn configured(&self) -> bool {
        self.api_key.is_some()
    }

    pub async fn token_info(&self, chain: Chain, address: &str) -> Result<Value, GmgnError> {
        self.get(
            "/v1/token/info",
            vec![
                ("chain", chain.gmgn_code().to_string()),
                ("address", address.to_string()),
            ],
        )
        .await
    }

    pub async fn token_security(&self, chain: Chain, address: &str) -> Result<Value, GmgnError> {
        self.get(
            "/v1/token/security",
            vec![
                ("chain", chain.gmgn_code().to_string()),
                ("address", address.to_string()),
            ],
        )
        .await
    }

    pub async fn token_pool(&self, chain: Chain, address: &str) -> Result<Value, GmgnError> {
        self.get(
            "/v1/token/pool_info",
            vec![
                ("chain", chain.gmgn_code().to_string()),
                ("address", address.to_string()),
            ],
        )
        .await
    }

    pub async fn top_holders(&self, chain: Chain, address: &str) -> Result<Value, GmgnError> {
        self.get(
            "/v1/market/token_top_holders",
            vec![
                ("chain", chain.gmgn_code().to_string()),
                ("address", address.to_string()),
                ("limit", "20".to_string()),
                ("order_by", "amount_percentage".to_string()),
                ("direction", "desc".to_string()),
            ],
        )
        .await
    }

    pub async fn top_traders(&self, chain: Chain, address: &str) -> Result<Value, GmgnError> {
        self.get(
            "/v1/market/token_top_traders",
            vec![
                ("chain", chain.gmgn_code().to_string()),
                ("address", address.to_string()),
                ("limit", "20".to_string()),
                ("order_by", "amount_percentage".to_string()),
                ("direction", "desc".to_string()),
            ],
        )
        .await
    }

    async fn get(
        &self,
        path: &str,
        mut params: Vec<(&'static str, String)>,
    ) -> Result<Value, GmgnError> {
        let api_key = self.api_key.as_ref().ok_or(GmgnError::NotConfigured)?;
        let url = format!("{}{}", self.host, path);

        for attempt in 0..2 {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| GmgnError::Transport(error.to_string()))?
                .as_secs();

            params.retain(|(key, _)| *key != "timestamp" && *key != "client_id");
            params.push(("timestamp", timestamp.to_string()));
            params.push(("client_id", Uuid::new_v4().to_string()));

            let response = self
                .http
                .get(&url)
                .header("X-APIKEY", api_key)
                .header("User-Agent", "water/0.1")
                .query(&params)
                .send()
                .await
                .map_err(|error| GmgnError::Transport(error.to_string()))?;

            let status = response.status();
            let body = response
                .text()
                .await
                .map_err(|error| GmgnError::Transport(error.to_string()))?;

            if status == StatusCode::TOO_MANY_REQUESTS && attempt == 0 {
                sleep(Duration::from_millis(1200)).await;
                continue;
            }

            if !status.is_success() {
                return Err(GmgnError::Http(status.as_u16(), body));
            }

            let envelope: Envelope =
                serde_json::from_str(&body).map_err(|_| GmgnError::InvalidResponse)?;

            if !code_is_zero(&envelope.code) {
                let detail = envelope
                    .error
                    .or(envelope.message)
                    .unwrap_or_else(|| format!("code={}", envelope.code));
                return Err(GmgnError::Api(detail));
            }

            return envelope.data.ok_or(GmgnError::InvalidResponse);
        }

        Err(GmgnError::InvalidResponse)
    }
}

fn code_is_zero(code: &Value) -> bool {
    match code {
        Value::Number(number) => number.as_i64() == Some(0),
        Value::String(value) => value == "0",
        _ => false,
    }
}
