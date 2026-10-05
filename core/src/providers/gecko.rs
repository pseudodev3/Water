use crate::model::{Chain, MarketSnapshot};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration as StdDuration, Instant},
};
use thiserror::Error;
use tokio::time::{sleep, Duration};

const SOL_WRAPPED_NATIVE: &str = "So11111111111111111111111111111111111111112";
const ROBINHOOD_WRAPPED_NATIVE: &str = "0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73";

#[derive(Clone)]
pub struct GeckoClient {
    http: reqwest::Client,
    host: String,
    pool_cache: Arc<Mutex<HashMap<String, String>>>,
    candle_cache: Arc<Mutex<HashMap<String, HashMap<u64, Decimal>>>>,
    market_cache: Arc<Mutex<HashMap<String, (Instant, MarketSnapshot)>>>,
    info_cache: Arc<Mutex<HashMap<String, (Instant, TokenInfoSnapshot)>>>,
    request_budget: Arc<Mutex<RequestBudget>>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PriceGranularity {
    Hour,
    Day,
}

#[derive(Clone, Debug, Serialize)]
pub struct TokenInfoSnapshot {
    pub image_url: Option<String>,
    pub websites: Vec<String>,
    pub twitter_url: Option<String>,
    pub telegram_url: Option<String>,
    pub discord_url: Option<String>,
    pub farcaster_url: Option<String>,
    pub zora_url: Option<String>,
    pub gt_verified: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct HistoricalPrice {
    pub usd_price: Decimal,
    pub granularity: PriceGranularity,
}

#[derive(Default)]
struct RequestBudget {
    starts: VecDeque<Instant>,
    cooldown_until: Option<Instant>,
}

impl RequestBudget {
    fn admit(&mut self, market: bool) -> Result<(), GeckoError> {
        let now = Instant::now();
        while self
            .starts
            .front()
            .is_some_and(|at| now.duration_since(*at) >= StdDuration::from_secs(60))
        {
            self.starts.pop_front();
        }
        if let Some(until) = self.cooldown_until.filter(|until| *until > now) {
            return Err(GeckoError::RateLimited(
                until.duration_since(now).as_secs().max(1),
            ));
        }
        // Reserve six requests for main scans; optional history must not spend
        // the whole public allowance. Cache hits never consume this budget.
        if self.starts.len() >= if market { 24 } else { 18 } {
            let remaining = self
                .starts
                .front()
                .map(|at| 60u64.saturating_sub(at.elapsed().as_secs()))
                .unwrap_or(60);
            return Err(GeckoError::RateLimited(remaining.max(1)));
        }
        self.starts.push_back(now);
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum GeckoError {
    #[error(
        "GeckoTerminal is rate-limited; retry in about {0}s. Chain evidence is still available."
    )]
    RateLimited(u64),
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
    pub async fn token_quotes(
        &self,
        chain: Chain,
        assets: &[String],
        at: u64,
    ) -> Result<Vec<crate::model::TokenQuote>, GeckoError> {
        let addresses: Vec<_> = assets
            .iter()
            .take(30)
            .map(|asset| market_asset_id(chain, asset))
            .collect();
        let payload = self
            .get_json(&format!(
                "{}/networks/{}/tokens/multi/{}",
                self.host,
                chain.market_network(),
                addresses.join(",")
            ))
            .await?;
        let rows = payload["data"]
            .as_array()
            .ok_or(GeckoError::InvalidResponse)?;
        Ok(assets
            .iter()
            .take(30)
            .filter_map(|asset| {
                let address = market_asset_id(chain, asset);
                let row = rows.iter().find(|row| {
                    let received = row["attributes"]["address"].as_str().unwrap_or("");
                    super::market::asset_key(chain, received)
                        == super::market::asset_key(chain, address)
                        && row["id"].as_str().is_some_and(|id| {
                            id.starts_with(&format!("{}_", chain.market_network()))
                        })
                })?;
                let attributes = &row["attributes"];
                let price = decimal_value(&attributes["price_usd"]).filter(|p| *p > Decimal::ZERO);
                Some(crate::model::TokenQuote {
                    asset: asset.clone(),
                    name: attributes["name"].as_str().map(str::to_owned),
                    symbol: attributes["symbol"].as_str().map(str::to_owned),
                    decimals: attributes["decimals"]
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok()),
                    price_usd: price,
                    observed_at: at,
                    source: "GeckoTerminal token market data".into(),
                    detail: if price.is_some() {
                        "Current indexed market mark; not an executable or historical trade price."
                    } else {
                        "Token identity received; GeckoTerminal returned no current USD price."
                    }
                    .into(),
                })
            })
            .collect())
    }
    pub fn new(http: reqwest::Client, host: String) -> Self {
        Self {
            http,
            host: host.trim_end_matches('/').to_string(),
            pool_cache: Arc::new(Mutex::new(HashMap::new())),
            candle_cache: Arc::new(Mutex::new(HashMap::new())),
            market_cache: Arc::new(Mutex::new(HashMap::new())),
            info_cache: Arc::new(Mutex::new(HashMap::new())),
            request_budget: Arc::new(Mutex::new(RequestBudget::default())),
        }
    }

    pub async fn market_snapshot(
        &self,
        chain: Chain,
        address: &str,
    ) -> Result<MarketSnapshot, GeckoError> {
        let network = chain.market_network();
        let cache_key = super::market::asset_key(chain, address);

        if let Some(snapshot) = self
            .market_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&cache_key).cloned())
            .and_then(|(fetched_at, snapshot)| {
                (fetched_at.elapsed() < StdDuration::from_secs(45)).then_some(snapshot)
            })
        {
            return Ok(snapshot);
        }

        // One public GeckoTerminal request is enough: token attributes provide
        // price/liquidity/volume and include=top_pools supplies the top-pool
        // transaction counts. This keeps scans well inside the public rate budget.
        let token_url = format!("{}/networks/{network}/tokens/{address}", self.host);
        let payload = self
            .send_json(
                self.http
                    .get(token_url)
                    .header("Accept", "application/json;version=20230203")
                    .header("User-Agent", "water/0.1")
                    .query(&[("include", "top_pools")]),
                true,
            )
            .await?;

        let snapshot = market_snapshot_from_payload(&payload)?;

        if let Ok(mut cache) = self.market_cache.lock() {
            cache.insert(cache_key, (Instant::now(), snapshot.clone()));
        }

        Ok(snapshot)
    }

    pub async fn token_info(
        &self,
        chain: Chain,
        address: &str,
    ) -> Result<TokenInfoSnapshot, GeckoError> {
        let network = chain.market_network();
        let cache_key = super::market::asset_key(chain, address);

        if let Some(info) = self
            .info_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&cache_key).cloned())
            .and_then(|(fetched_at, info)| {
                (fetched_at.elapsed() < StdDuration::from_secs(600)).then_some(info)
            })
        {
            return Ok(info);
        }

        let url = format!("{}/networks/{network}/tokens/{address}/info", self.host);
        let payload = self.get_json(&url).await?;
        let info = token_info_from_payload(&payload)?;

        if let Ok(mut cache) = self.info_cache.lock() {
            cache.insert(cache_key, (Instant::now(), info.clone()));
        }

        Ok(info)
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
        let asset_key = super::market::asset_key(chain, token);

        let pool_address = if let Some(cached) = self
            .pool_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&asset_key).cloned())
        {
            cached
        } else {
            let pools_url = format!("{}/networks/{network}/tokens/{token}/pools", self.host);
            let pools = self.get_json(&pools_url).await?;
            let discovered = first_pool_address(&pools).ok_or(GeckoError::NoData)?;
            if let Ok(mut cache) = self.pool_cache.lock() {
                cache.insert(asset_key.clone(), discovered.clone());
            }
            discovered
        };

        let hourly_key = format!("{asset_key}:hour");
        let daily_key = format!("{asset_key}:day");
        let needs_hourly = {
            let cache = self.candle_cache.lock().ok();
            timestamps.iter().any(|timestamp| {
                let hour = timestamp / 3_600 * 3_600;
                cache
                    .as_ref()
                    .and_then(|cache| cache.get(&hourly_key))
                    .is_none_or(|prices| !prices.contains_key(&hour))
            })
        };
        let needs_daily = {
            let cache = self.candle_cache.lock().ok();
            timestamps.iter().any(|timestamp| {
                let day = timestamp / 86_400 * 86_400;
                cache
                    .as_ref()
                    .and_then(|cache| cache.get(&daily_key))
                    .is_none_or(|prices| !prices.contains_key(&day))
            })
        };

        let max_timestamp = timestamps.iter().copied().max().ok_or(GeckoError::NoData)?;

        if needs_hourly {
            let hourly = self
                .ohlcv(
                    network,
                    &pool_address,
                    "hour",
                    token,
                    max_timestamp.saturating_add(3_600),
                )
                .await?;
            if let Ok(mut cache) = self.candle_cache.lock() {
                cache
                    .entry(hourly_key.clone())
                    .or_default()
                    .extend(candle_map(&hourly));
            }
        }

        if needs_daily {
            let daily = self
                .ohlcv(
                    network,
                    &pool_address,
                    "day",
                    token,
                    max_timestamp.saturating_add(86_400),
                )
                .await?;
            if let Ok(mut cache) = self.candle_cache.lock() {
                cache
                    .entry(daily_key.clone())
                    .or_default()
                    .extend(candle_map(&daily));
            }
        }

        let cache = self
            .candle_cache
            .lock()
            .map_err(|_| GeckoError::InvalidResponse)?;
        let hourly_prices = cache.get(&hourly_key);
        let daily_prices = cache.get(&daily_key);
        let mut resolved = HashMap::new();

        for timestamp in timestamps {
            let hour = timestamp / 3_600 * 3_600;
            let day = timestamp / 86_400 * 86_400;

            if let Some(price) = hourly_prices.and_then(|prices| prices.get(&hour)).copied() {
                resolved.insert(
                    *timestamp,
                    HistoricalPrice {
                        usd_price: price,
                        granularity: PriceGranularity::Hour,
                    },
                );
            } else if let Some(price) = daily_prices.and_then(|prices| prices.get(&day)).copied() {
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

        self.send_json(
            self.http
                .get(url)
                .header("Accept", "application/json;version=20230203")
                .header("User-Agent", "water/0.1")
                .query(&[
                    ("aggregate", "1".to_string()),
                    ("before_timestamp", before_timestamp.to_string()),
                    ("limit", "1000".to_string()),
                    ("currency", "usd".to_string()),
                    ("token", token.to_string()),
                ]),
            false,
        )
        .await
    }

    async fn get_json(&self, url: &str) -> Result<Value, GeckoError> {
        self.send_json(
            self.http
                .get(url)
                .header("Accept", "application/json;version=20230203")
                .header("User-Agent", "water/0.1"),
            false,
        )
        .await
    }

    async fn send_json(
        &self,
        request: reqwest::RequestBuilder,
        market: bool,
    ) -> Result<Value, GeckoError> {
        let mut last_error = None;
        for attempt in 0..2u64 {
            self.request_budget
                .lock()
                .map_err(|_| GeckoError::InvalidResponse)?
                .admit(market)?;
            let request = request
                .try_clone()
                .ok_or(GeckoError::InvalidResponse)?
                .timeout(StdDuration::from_secs(3));
            match request.send().await {
                Ok(response) => {
                    let status = response.status();
                    if status.as_u16() == 429 {
                        let retry_after = response
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(60)
                            .clamp(1, 300);
                        if let Ok(mut budget) = self.request_budget.lock() {
                            budget.cooldown_until =
                                Some(Instant::now() + StdDuration::from_secs(retry_after));
                        }
                        // Repeated requests from the same IP do not repair a 429.
                        return Err(GeckoError::RateLimited(retry_after));
                    }
                    if !status.is_success() {
                        let detail = if status.as_u16() == 404 {
                            "No indexed listing for this token."
                        } else {
                            status.canonical_reason().unwrap_or("Provider unavailable")
                        };
                        last_error = Some(GeckoError::Http(status.as_u16(), detail.to_string()));
                        if !status.is_server_error() {
                            return Err(last_error.unwrap());
                        }
                    } else {
                        let body = response
                            .text()
                            .await
                            .map_err(|e| GeckoError::Transport(e.without_url().to_string()))?;
                        return serde_json::from_str(&body)
                            .map_err(|_| GeckoError::InvalidResponse);
                    }
                }
                Err(error) => {
                    last_error = Some(GeckoError::Transport(error.without_url().to_string()))
                }
            }
            if attempt == 0 {
                sleep(Duration::from_millis(250)).await;
            }
        }
        Err(last_error.unwrap_or(GeckoError::NoData))
    }
}

fn token_info_from_payload(payload: &Value) -> Result<TokenInfoSnapshot, GeckoError> {
    let attributes = payload
        .pointer("/data/attributes")
        .and_then(Value::as_object)
        .ok_or(GeckoError::NoData)?;

    let websites = attributes
        .get("websites")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .filter_map(safe_external_url)
                .take(3)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let twitter_url = string_field(attributes.get("twitter_handle"))
        .and_then(|value| normalize_social_handle(&value, "https://x.com/"));
    let telegram_url = string_field(attributes.get("telegram_handle"))
        .and_then(|value| normalize_social_handle(&value, "https://t.me/"));

    Ok(TokenInfoSnapshot {
        image_url: string_field(attributes.get("image_url"))
            .and_then(|value| safe_external_url(&value)),
        websites,
        twitter_url,
        telegram_url,
        discord_url: string_field(attributes.get("discord_url"))
            .and_then(|value| safe_external_url(&value)),
        farcaster_url: string_field(attributes.get("farcaster_url"))
            .and_then(|value| safe_external_url(&value)),
        zora_url: string_field(attributes.get("zora_url"))
            .and_then(|value| safe_external_url(&value)),
        gt_verified: attributes.get("gt_verified").and_then(Value::as_bool),
    })
}

fn safe_external_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let parsed = reqwest::Url::parse(trimmed).ok()?;
    match parsed.scheme() {
        "http" | "https" => Some(parsed.to_string()),
        _ => None,
    }
}

fn normalize_social_handle(value: &str, base: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return safe_external_url(trimmed);
    }

    let handle = trimmed.trim_start_matches('@').trim_start_matches('/');
    (!handle.is_empty()).then(|| format!("{base}{handle}"))
}

fn market_snapshot_from_payload(payload: &Value) -> Result<MarketSnapshot, GeckoError> {
    let attributes = payload
        .pointer("/data/attributes")
        .and_then(Value::as_object)
        .ok_or(GeckoError::NoData)?;

    let first_pool = payload
        .get("included")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("type").and_then(Value::as_str) == Some("pool"))
        })
        .and_then(|row| row.get("attributes"));

    Ok(MarketSnapshot {
        basis: Some("geckoterminal:aggregate".to_string()),
        name: string_field(attributes.get("name")),
        symbol: string_field(attributes.get("symbol")),
        price_usd: positive_number_field(attributes.get("price_usd")),
        liquidity_usd: number_field(attributes.get("total_reserve_in_usd"))
            .or_else(|| first_pool.and_then(|pool| number_path(pool, &["reserve_in_usd"]))),
        market_cap_usd: positive_number_field(attributes.get("market_cap_usd")),
        fdv_usd: positive_number_field(attributes.get("fdv_usd")),
        volume_h24_usd: attributes
            .get("volume_usd")
            .and_then(|value| number_path(value, &["h24"]))
            .or_else(|| first_pool.and_then(|pool| number_path(pool, &["volume_usd", "h24"]))),
        buys_h1: first_pool.and_then(|pool| integer_path(pool, &["transactions", "h1", "buys"])),
        sells_h1: first_pool.and_then(|pool| integer_path(pool, &["transactions", "h1", "sells"])),
        buyers_h1: first_pool
            .and_then(|pool| integer_path(pool, &["transactions", "h1", "buyers"])),
        sellers_h1: first_pool
            .and_then(|pool| integer_path(pool, &["transactions", "h1", "sellers"])),
        buyers_h24: first_pool
            .and_then(|pool| integer_path(pool, &["transactions", "h24", "buyers"])),
        sellers_h24: first_pool
            .and_then(|pool| integer_path(pool, &["transactions", "h24", "sellers"])),
    })
}

pub(crate) fn market_asset_id(chain: Chain, asset_id: &str) -> &str {
    match chain {
        Chain::Solana if asset_id == "SOL" => SOL_WRAPPED_NATIVE,
        Chain::Robinhood if asset_id.eq_ignore_ascii_case("ETH") => ROBINHOOD_WRAPPED_NATIVE,
        Chain::Bnb if asset_id.eq_ignore_ascii_case("BNB") => {
            "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c"
        }
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

fn positive_number_field(value: Option<&Value>) -> Option<f64> {
    number_field(value).filter(|value| value.is_finite() && *value > 0.0)
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
    fn optional_history_reserves_market_budget_and_cooldown_expires() {
        let mut budget = RequestBudget::default();
        for _ in 0..18 {
            budget.admit(false).unwrap();
        }
        assert!(matches!(
            budget.admit(false),
            Err(GeckoError::RateLimited(_))
        ));
        for _ in 0..6 {
            budget.admit(true).unwrap();
        }
        assert!(matches!(
            budget.admit(true),
            Err(GeckoError::RateLimited(_))
        ));
        budget.starts.clear();
        budget.cooldown_until = Some(Instant::now() + StdDuration::from_secs(60));
        assert!(matches!(
            budget.admit(true),
            Err(GeckoError::RateLimited(_))
        ));
        budget.cooldown_until = Some(Instant::now() - StdDuration::from_secs(1));
        budget.admit(true).unwrap();
    }

    #[test]
    fn zero_token_valuation_does_not_use_an_unrelated_pool_valuation() {
        let payload = serde_json::json!({
            "data":{"attributes":{"price_usd":"0.002","market_cap_usd":"0","fdv_usd":"NaN","volume_usd":{"h24":"0"}}},
            "included":[{"type":"pool","attributes":{"market_cap_usd":"999999999","fdv_usd":"999999999"}}]
        });
        let market = market_snapshot_from_payload(&payload).unwrap();
        assert!(market.market_cap_usd.is_none());
        assert!(market.fdv_usd.is_none());
        assert_eq!(market.volume_h24_usd, Some(0.0));
    }

    #[test]
    fn token_payload_with_included_top_pool_builds_complete_market_snapshot() {
        let payload = json!({
            "data": {
                "type": "token",
                "attributes": {
                    "name": "Example",
                    "symbol": "EX",
                    "price_usd": "0.002",
                    "total_reserve_in_usd": "50000",
                    "market_cap_usd": null,
                    "fdv_usd": "2000000",
                    "volume_usd": {"h24": "125000"}
                }
            },
            "included": [{
                "type": "pool",
                "attributes": {
                    "reserve_in_usd": "49000",
                    "transactions": {
                        "h1": {"buys": 14, "sells": 9, "buyers": 10, "sellers": 7},
                        "h24": {"buys": 200, "sells": 180, "buyers": 120, "sellers": 105}
                    },
                    "volume_usd": {"h24": "124000"}
                }
            }]
        });

        let snapshot = market_snapshot_from_payload(&payload).unwrap();

        assert_eq!(snapshot.name.as_deref(), Some("Example"));
        assert_eq!(snapshot.symbol.as_deref(), Some("EX"));
        assert_eq!(snapshot.price_usd, Some(0.002));
        assert_eq!(snapshot.liquidity_usd, Some(50_000.0));
        assert_eq!(snapshot.fdv_usd, Some(2_000_000.0));
        assert_eq!(snapshot.buys_h1, Some(14));
        assert_eq!(snapshot.sells_h1, Some(9));
        assert_eq!(snapshot.buyers_h1, Some(10));
        assert_eq!(snapshot.sellers_h1, Some(7));
        assert_eq!(snapshot.buyers_h24, Some(120));
        assert_eq!(snapshot.sellers_h24, Some(105));
    }

    #[test]
    fn token_info_normalizes_provider_links() {
        let payload = json!({
            "data": {
                "attributes": {
                    "image_url": "https://assets.example/token.png",
                    "websites": ["https://example.com", "javascript:alert(1)"],
                    "twitter_handle": "@example_token",
                    "telegram_handle": "example_chat",
                    "discord_url": "https://discord.gg/example",
                    "farcaster_url": null,
                    "zora_url": null,
                    "gt_verified": true
                }
            }
        });

        let info = token_info_from_payload(&payload).unwrap();
        assert_eq!(info.websites.len(), 1);
        assert_eq!(
            info.twitter_url.as_deref(),
            Some("https://x.com/example_token")
        );
        assert_eq!(
            info.telegram_url.as_deref(),
            Some("https://t.me/example_chat")
        );
        assert_eq!(info.gt_verified, Some(true));
    }

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
        assert_eq!(market_asset_id(Chain::Solana, "SOL"), SOL_WRAPPED_NATIVE);
        assert_eq!(
            market_asset_id(Chain::Robinhood, "ETH"),
            ROBINHOOD_WRAPPED_NATIVE
        );
    }
}
