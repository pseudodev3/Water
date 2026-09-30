use crate::model::{Chain, MarketSnapshot};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone)]
pub struct GeckoClient {
    http: reqwest::Client,
    host: String,
}

#[derive(Debug, Error)]
pub enum GeckoError {
    #[error("GeckoTerminal request failed: {0}")]
    Transport(String),
    #[error("GeckoTerminal returned HTTP {0}: {1}")]
    Http(u16, String),
    #[error("GeckoTerminal returned no market data for this token")]
    NoData,
    #[error("GeckoTerminal returned an unreadable response")]
    InvalidResponse,
}

impl GeckoClient {
    pub fn new(http: reqwest::Client, host: String) -> Self {
        Self {
            http,
            host: host.trim_end_matches('/').to_string(),
        }
    }

    pub async fn market_snapshot(
        &self,
        chain: Chain,
        address: &str,
    ) -> Result<MarketSnapshot, GeckoError> {
        let network = chain.market_network();
        let token_url = format!("{}/networks/{network}/tokens/{address}", self.host);
        let pools_url = format!("{}/networks/{network}/tokens/{address}/pools", self.host);

        let token_request = self.get_json(&token_url);
        let pools_request = self.get_json(&pools_url);
        let (token, pools) = tokio::join!(token_request, pools_request);

        let token = token?;
        let pools = pools?;

        let attributes = token
            .pointer("/data/attributes")
            .and_then(Value::as_object)
            .ok_or(GeckoError::NoData)?;

        let first_pool = pools
            .get("data")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("attributes"));

        Ok(MarketSnapshot {
            name: string_field(attributes.get("name")),
            symbol: string_field(attributes.get("symbol")),
            price_usd: number_field(attributes.get("price_usd")),
            liquidity_usd: number_field(attributes.get("total_reserve_in_usd"))
                .or_else(|| first_pool.and_then(|pool| number_path(pool, &["reserve_in_usd"]))),
            market_cap_usd: number_field(attributes.get("market_cap_usd")),
            fdv_usd: number_field(attributes.get("fdv_usd")),
            volume_h24_usd: attributes
                .get("volume_usd")
                .and_then(|value| number_path(value, &["h24"])),
            buys_h1: first_pool
                .and_then(|pool| integer_path(pool, &["transactions", "h1", "buys"])),
            sells_h1: first_pool
                .and_then(|pool| integer_path(pool, &["transactions", "h1", "sells"])),
        })
    }

    async fn get_json(&self, url: &str) -> Result<Value, GeckoError> {
        let response = self
            .http
            .get(url)
            .header("Accept", "application/json;version=20230203")
            .header("User-Agent", "water/0.1")
            .send()
            .await
            .map_err(|error| GeckoError::Transport(error.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| GeckoError::Transport(error.to_string()))?;

        if !status.is_success() {
            return Err(GeckoError::Http(status.as_u16(), body));
        }

        serde_json::from_str(&body).map_err(|_| GeckoError::InvalidResponse)
    }
}

fn string_field(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn number_field(value: Option<&Value>) -> Option<f64> {
    value.and_then(value_as_f64)
}

fn number_path(value: &Value, path: &[&str]) -> Option<f64> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(value_as_f64)
}

fn integer_path(value: &Value, path: &[&str]) -> Option<u64> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(|value| match value {
            Value::Number(number) => number.as_u64(),
            Value::String(value) => value.parse::<u64>().ok(),
            _ => None,
        })
}

fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(value) => value.replace(',', "").parse::<f64>().ok(),
        _ => None,
    }
}
