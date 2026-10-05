//! Independent USD candles for native-coin conversions when DEX candles fail.
//! These are indicative exchange marks, never onchain execution prices.
use crate::model::Chain;
use rust_decimal::Decimal;
use serde_json::Value;
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Mutex as AsyncMutex;

#[derive(Clone)]
pub struct NativePriceClient {
    http: reqwest::Client,
    host: String,
    cache: Arc<Mutex<HashMap<String, (Instant, Vec<(u64, Decimal)>)>>>,
    gate: Arc<AsyncMutex<Instant>>,
}
pub struct NativeCandle {
    pub usd: Decimal,
    pub granularity: String,
    pub source: String,
}
impl NativePriceClient {
    pub fn new(http: reqwest::Client, host: String) -> Self {
        Self {
            http,
            host,
            cache: Arc::new(Mutex::new(HashMap::new())),
            gate: Arc::new(AsyncMutex::new(Instant::now())),
        }
    }
    pub async fn historical(
        &self,
        chain: Chain,
        asset: &str,
        times: &[u64],
        at: u64,
    ) -> Result<HashMap<u64, NativeCandle>, String> {
        let Some((pair, response_key, label)) = native_pair(chain, asset) else {
            return Ok(HashMap::new());
        };
        let mut output = HashMap::new();
        for (interval, seconds, granularity) in [(60, 3600, "hour"), (1440, 86400, "day")] {
            if times.iter().all(|t| output.contains_key(t)) {
                break;
            }
            let rows = self.candles(pair, response_key, interval).await?;
            let index: HashMap<_, _> = rows
                .into_iter()
                .filter(|(t, _)| t.saturating_add(seconds) <= at)
                .collect();
            for time in times {
                if let Some(usd) = index.get(&(time / seconds * seconds)) {
                    output.entry(*time).or_insert_with(|| NativeCandle {
                        usd: *usd,
                        granularity: granularity.into(),
                        source: format!(
                            "Kraken {label}/USD historical candle; fallback indicative conversion"
                        ),
                    });
                }
            }
        }
        Ok(output)
    }
    async fn candles(
        &self,
        pair: &str,
        response_key: &str,
        interval: u64,
    ) -> Result<Vec<(u64, Decimal)>, String> {
        let key = format!("{pair}:{interval}");
        let cached = || {
            self.cache
                .lock()
                .ok()
                .and_then(|c| c.get(&key).cloned())
                .filter(|(at, _)| at.elapsed() < Duration::from_secs(600))
                .map(|(_, v)| v)
        };
        if let Some(rows) = cached() {
            return Ok(rows);
        }
        let mut gate = self.gate.lock().await;
        if let Some(rows) = cached() {
            return Ok(rows);
        }
        let elapsed = gate.elapsed();
        if elapsed < Duration::from_millis(1200) {
            tokio::time::sleep(Duration::from_millis(1200) - elapsed).await;
        }
        *gate = Instant::now();
        let response = self
            .http
            .get(format!("{}/0/public/OHLC", self.host.trim_end_matches('/')))
            .query(&[
                ("pair", pair.to_string()),
                ("interval", interval.to_string()),
            ])
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .map_err(|_| "Kraken candle transport unavailable.")?;
        let response = response.error_for_status().map_err(|e| {
            format!(
                "Kraken candles returned HTTP {}.",
                e.status().map(|s| s.as_u16()).unwrap_or(0)
            )
        })?;
        let value: Value = response
            .json()
            .await
            .map_err(|_| "Kraken candles returned unreadable JSON.")?;
        let rows = parse_candles(&value, response_key)?;
        self.cache
            .lock()
            .map_err(|_| "Native price cache is busy.")?
            .insert(key, (Instant::now(), rows.clone()));
        Ok(rows)
    }
}
fn native_pair(chain: Chain, asset: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match chain {
        Chain::Solana if matches!(asset, "SOL" | "So11111111111111111111111111111111111111112") => {
            Some(("SOLUSD", "SOLUSD", "SOL"))
        }
        Chain::Robinhood
            if asset.eq_ignore_ascii_case("ETH")
                || asset.eq_ignore_ascii_case("0x0bd7d308f8e1639fab988df18a8011f41eacad73") =>
        {
            Some(("ETHUSD", "XETHZUSD", "ETH"))
        }
        Chain::Bnb
            if asset.eq_ignore_ascii_case("BNB")
                || asset.eq_ignore_ascii_case("0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c") =>
        {
            Some(("BNBUSD", "BNBUSD", "BNB"))
        }
        _ => None,
    }
}
fn parse_candles(value: &Value, key: &str) -> Result<Vec<(u64, Decimal)>, String> {
    if !value["error"]
        .as_array()
        .is_some_and(|errors| errors.is_empty())
    {
        return Err("Kraken rejected the historical candle request.".into());
    }
    let rows = value["result"][key]
        .as_array()
        .ok_or("Kraken omitted the requested USD pair.")?;
    // The official API always includes an uncommitted final row.
    Ok(rows
        .iter()
        .take(rows.len().saturating_sub(1))
        .filter_map(|row| {
            let time = row[0].as_u64()?;
            let close = Decimal::from_str(row[4].as_str()?).ok()?;
            (close > Decimal::ZERO).then_some((time, close))
        })
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn only_verified_native_aliases_use_the_usd_pair_and_forming_bars_are_excluded() {
        assert!(native_pair(Chain::Bnb, "SOL").is_none());
        assert!(native_pair(
            Chain::Robinhood,
            "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c"
        )
        .is_none());
        let data = json!({"error":[],"result":{"SOLUSD":[[3600,"1","1","1","0.0000000000123456789"],[7200,"2","2","2","999"]]}});
        let rows = parse_candles(&data, "SOLUSD").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.to_string(), "0.0000000000123456789");
        assert!(parse_candles(&data, "XETHZUSD").is_err());
        assert!(parse_candles(&json!({"error":["EAPI:Rate limit exceeded"]}), "SOLUSD").is_err());
    }
}
