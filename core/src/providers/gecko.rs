use crate::model::{Chain, MarketSnapshot};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashMap, str::FromStr};
use thiserror::Error;

const SOL_WRAPPED_NATIVE: &str = "So11111111111111111111111111111111111111112";
const ROBINHOOD_WRAPPED_NATIVE: &str = "0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73";

#[derive(Clone)]
pub struct GeckoClient {
    http: reqwest::Client,
    host: String,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PriceGranularity {
    Hour,
    Day,
}

#[derive(Clone, Debug)]
pub struct HistoricalPrice {
    pub usd_price: Decimal,
    pub granularity: PriceGranularity,
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

    /// Resolve many transaction timestamps with at most three public API calls:
    /// pool discovery + 1000 hourly candles + 1000 daily candles.
    ///
    /// Hourly candles are preferred. Daily candles are an explicit lower-precision
    /// fallback for older history. Missing candles remain missing.
    pub async fn historical_usd_prices(
        &self,
        chain: Chain,
        asset_id: &str,
        timestamps: &[u64],
    ) -> Result<HashMap<u64, HistoricalPrice>, GeckoError> {
        if timestamps.is_empty() {
            return Ok(HashMap::new());
        }

        let token = market_asset_id(chain, asset_id);
        let network = chain.market_network();
        let pools_url = format!("{}/networks/{network}/tokens/{token}/pools", self.host);
        let pools = self.get_json(&pools_url).await?;
        let pool_address = first_pool_address(&pools).ok_or(GeckoError::NoData)?;

        let max_timestamp = timestamps.iter().copied().max().ok_or(GeckoError::NoData)?;
        let hourly = self
            .ohlcv(
                network,
                &pool_address,
                "hour",
                token,
                max_timestamp.saturating_add(3_600),
            )
            .await?;
        let daily = self
            .ohlcv(
                network,
                &pool_address,
                "day",
                token,
                max_timestamp.saturating_add(86_400),
            )
            .await?;

        let hourly_prices = candle_map(&hourly);
        let daily_prices = candle_map(&daily);
        let mut resolved = HashMap::new();

        for timestamp in timestamps {
            let hour = timestamp / 3_600 * 3_600;
            let day = timestamp / 86_400 * 86_400;

            if let Some(price) = hourly_prices.get(&hour).copied() {
                resolved.insert(
                    *timestamp,
                    HistoricalPrice {
                        usd_price: price,
                        granularity: PriceGranularity::Hour,
                    },
                );
            } else if let Some(price) = daily_prices.get(&day).copied() {
                resolved.insert(
                    *timestamp,
                    HistoricalPrice {
                        usd_price: price,
                        granularity: PriceGranularity::Day,
                    },
                );
            }
        }

        Ok(resolved)
    }

    async fn ohlcv(
        &self,
        network: &str,
        pool_address: &str,
        timeframe: &str,
        token: &str,
        before_timestamp: u64,
    ) -> Result<Value, GeckoError> {
        let url = format!(
            "{}/networks/{network}/pools/{pool_address}/ohlcv/{timeframe}",
            self.host
        );

        let response = self
            .http
            .get(url)
            .header("Accept", "application/json;version=20230203")
            .header("User-Agent", "water/0.1")
            .query(&[
                ("aggregate", "1".to_string()),
                ("before_timestamp", before_timestamp.to_string()),
                ("limit", "1000".to_string()),
                ("currency", "usd".to_string()),
                ("token", token.to_string()),
            ])
            .send()
            .await
            .map_err(|error| GeckoError::Transport(error.to_string()))?;

        parse_response(response).await
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

        parse_response(response).await
    }
}

async fn parse_response(response: reqwest::Response) -> Result<Value, GeckoError> {
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

fn market_asset_id(chain: Chain, asset_id: &str) -> &str {
    match chain {
        Chain::Solana if asset_id == "SOL" => SOL_WRAPPED_NATIVE,
        Chain::Robinhood if asset_id.eq_ignore_ascii_case("ETH") => ROBINHOOD_WRAPPED_NATIVE,
        _ => asset_id,
    }
}

fn first_pool_address(pools: &Value) -> Option<String> {
    let row = pools.get("data")?.as_array()?.first()?;

    row.pointer("/attributes/address")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            row.get("id")
                .and_then(Value::as_str)
                .and_then(|id| id.split_once('_').map(|(_, address)| address.to_string()))
        })
}

fn candle_map(payload: &Value) -> HashMap<u64, Decimal> {
    payload
        .pointer("/data/attributes/ohlcv_list")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let values = row.as_array()?;
                    let timestamp = values.first()?.as_u64()?;
                    let close = values.get(4).and_then(decimal_value)?;
                    Some((timestamp, close))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn decimal_value(value: &Value) -> Option<Decimal> {
    match value {
        Value::Number(number) => Decimal::from_str(&number.to_string()).ok(),
        Value::String(value) => Decimal::from_str(&value.replace(',', "")).ok(),
        _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn candle_parser_uses_close_price() {
        let payload = json!({
            "data": {
                "attributes": {
                    "ohlcv_list": [
                        [3600, 1.0, 3.0, 0.5, 2.5, 1000.0]
                    ]
                }
            }
        });

        let prices = candle_map(&payload);
        assert_eq!(prices.get(&3600), Some(&Decimal::new(25, 1)));
    }

    #[test]
    fn native_assets_map_to_wrapped_market_assets() {
        assert_eq!(
            market_asset_id(Chain::Solana, "SOL"),
            SOL_WRAPPED_NATIVE
        );
        assert_eq!(
            market_asset_id(Chain::Robinhood, "ETH"),
            ROBINHOOD_WRAPPED_NATIVE
        );
    }
}
