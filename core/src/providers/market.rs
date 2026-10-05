//! Public market reads: GeckoTerminal first, independently labelled fallback.
use super::gecko::GeckoClient;
use crate::model::{Chain, MarketSnapshot, SourceStatus};
use reqwest::Client;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::sync::Mutex as AsyncMutex;

#[derive(Clone)]
pub struct MarketRead {
    pub snapshot: Option<MarketSnapshot>,
    pub sources: Vec<SourceStatus>,
}

#[derive(Clone)]
pub struct MarketClient {
    http: Client,
    gecko: GeckoClient,
    fallback_host: String,
    cache: Arc<Mutex<HashMap<String, (Instant, MarketRead)>>>,
    pending: Arc<Mutex<HashMap<String, Weak<AsyncMutex<()>>>>>,
}

pub fn asset_key(chain: Chain, address: &str) -> String {
    // Base58 is case-sensitive. Only EVM addresses may be lowercased.
    let address = match chain {
        Chain::Solana => address.to_string(),
        Chain::Robinhood | Chain::Bnb => address.to_ascii_lowercase(),
    };
    format!("{}:{address}", chain.market_network())
}

impl MarketClient {
    pub async fn token_quotes(
        &self,
        chain: Chain,
        assets: &[String],
        at: u64,
    ) -> Result<Vec<crate::model::TokenQuote>, String> {
        let payload: Value = self
            .http
            .get(format!(
                "{}/tokens/v1/{}/{}",
                self.fallback_host,
                chain.market_network(),
                assets
                    .iter()
                    .take(30)
                    .map(|a| super::gecko::market_asset_id(chain, a))
                    .collect::<Vec<_>>()
                    .join(",")
            ))
            .timeout(Duration::from_secs(4))
            .send()
            .await
            .map_err(|_| "Dexscreener batch transport unavailable.")?
            .error_for_status()
            .map_err(|e| {
                format!(
                    "Dexscreener batch returned HTTP {}.",
                    e.status().map(|s| s.as_u16()).unwrap_or(0)
                )
            })?
            .json()
            .await
            .map_err(|_| "Dexscreener batch returned unreadable JSON.")?;
        Ok(batch_quotes(&payload, chain, assets, at))
    }
    pub fn new(http: Client, gecko: GeckoClient, fallback_host: String) -> Self {
        Self {
            http,
            gecko,
            fallback_host: fallback_host.trim_end_matches('/').to_string(),
            cache: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn cached(&self, key: &str) -> Option<MarketRead> {
        self.cache
            .lock()
            .ok()?
            .get(key)
            .cloned()
            .and_then(|(at, read)| {
                // Failed reads are only shared briefly; a retry can recover.
                let ttl = if read.snapshot.is_some() { 45 } else { 1 };
                (at.elapsed() < Duration::from_secs(ttl)).then_some(read)
            })
    }

    pub async fn snapshot(&self, chain: Chain, address: &str) -> MarketRead {
        let key = asset_key(chain, address);
        if let Some(read) = self.cached(&key) {
            return read;
        }
        let gate = {
            let mut pending = self.pending.lock().unwrap();
            pending.retain(|_, gate| gate.strong_count() > 0);
            pending
                .get(&key)
                .and_then(Weak::upgrade)
                .unwrap_or_else(|| {
                    let gate = Arc::new(AsyncMutex::new(()));
                    pending.insert(key.clone(), Arc::downgrade(&gate));
                    gate
                })
        };
        let _guard = gate.lock().await;
        if let Some(read) = self.cached(&key) {
            return read;
        }

        let primary = tokio::time::timeout(
            Duration::from_secs(7),
            self.gecko.market_snapshot(chain, address),
        )
        .await;
        let detail = match primary {
            Ok(Ok(snapshot)) => {
                let read = MarketRead {
                    snapshot: Some(snapshot),
                    sources: vec![SourceStatus {
                        source: "GeckoTerminal public market data".to_string(),
                        ok: true,
                        detail: "Token and top-pool market data received.".to_string(),
                    }],
                };
                self.store(key, read.clone());
                return read;
            }
            Ok(Err(error)) => error.to_string(),
            Err(_) => "GeckoTerminal did not answer within its 7s budget.".to_string(),
        };
        let mut sources = vec![SourceStatus {
            source: "GeckoTerminal public market data".to_string(),
            ok: false,
            detail,
        }];
        let fallback =
            tokio::time::timeout(Duration::from_secs(4), self.fallback(chain, address)).await;
        let snapshot = match fallback {
            Ok(Ok((snapshot, pool))) => {
                sources.push(SourceStatus {
                    source: "Dexscreener public market data (fallback)".to_string(), ok: true,
                    detail: format!("Price, liquidity and transaction counts come from the deepest indexed pool {pool}. Liquidity and volume describe this pool. Unique buyer/seller counts are unavailable from this source."),
                });
                Some(snapshot)
            }
            result => {
                let detail = match result {
                    Ok(Err(error)) => error,
                    Err(_) => "Dexscreener did not answer within its 4s budget.".to_string(),
                    _ => unreachable!(),
                };
                sources.push(SourceStatus {
                    source: "Dexscreener public market data (fallback)".to_string(),
                    ok: false,
                    detail,
                });
                None
            }
        };
        let read = MarketRead { snapshot, sources };
        self.store(key, read.clone());
        read
    }

    fn store(&self, key: String, read: MarketRead) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(45));
            cache.insert(key, (Instant::now(), read));
        }
    }

    async fn fallback(
        &self,
        chain: Chain,
        address: &str,
    ) -> Result<(MarketSnapshot, String), String> {
        let response = self
            .http
            .get(format!(
                "{}/token-pairs/v1/{}/{address}",
                self.fallback_host,
                chain.market_network()
            ))
            .header("Accept", "application/json")
            .header("User-Agent", "water/0.1")
            .timeout(Duration::from_secs(4))
            .send()
            .await
            .map_err(|e| format!("Dexscreener request failed: {}", e.without_url()))?;
        if !response.status().is_success() {
            return Err(format!(
                "Dexscreener returned HTTP {}.",
                response.status().as_u16()
            ));
        }
        let payload: Value = response
            .json()
            .await
            .map_err(|_| "Dexscreener returned an unreadable response.".to_string())?;
        fallback_snapshot(&payload, chain, address).ok_or_else(|| {
            "Dexscreener has no priced base-token pool for this exact token on this chain."
                .to_string()
        })
    }
}

fn batch_quotes(
    payload: &Value,
    chain: Chain,
    assets: &[String],
    at: u64,
) -> Vec<crate::model::TokenQuote> {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    assets
        .iter()
        .filter_map(|asset| {
            let row = payload
                .as_array()?
                .iter()
                .filter(|row| {
                    row["chainId"].as_str() == Some(chain.market_network())
                        && row["baseToken"]["address"].as_str().is_some_and(|a| {
                            asset_key(chain, a)
                                == asset_key(chain, super::gecko::market_asset_id(chain, asset))
                        })
                        && row["priceUsd"]
                            .as_str()
                            .and_then(|s| Decimal::from_str(s).ok())
                            .is_some_and(|p| p > Decimal::ZERO)
                })
                .max_by(|a, b| {
                    let liquidity = |row: &Value| {
                        row["liquidity"]["usd"]
                            .as_number()
                            .and_then(|n| Decimal::from_str(&n.to_string()).ok())
                            .unwrap_or_default()
                    };
                    liquidity(a).cmp(&liquidity(b))
                })?;
            Some(crate::model::TokenQuote {
                asset: asset.clone(),
                name: row["baseToken"]["name"].as_str().map(str::to_owned),
                symbol: row["baseToken"]["symbol"].as_str().map(str::to_owned),
                decimals: None,
                price_usd: row["priceUsd"]
                    .as_str()
                    .and_then(|s| Decimal::from_str(s).ok()),
                observed_at: at,
                source: "Dexscreener deepest indexed base-token pool".into(),
                detail: "Current indexed market mark; not an executable or historical trade price."
                    .into(),
            })
        })
        .collect()
}

fn number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
        .filter(|n| n.is_finite() && *n >= 0.0)
}
fn positive(value: Option<&Value>) -> Option<f64> {
    number(value).filter(|n| *n > 0.0)
}

fn fallback_snapshot(
    payload: &Value,
    chain: Chain,
    token: &str,
) -> Option<(MarketSnapshot, String)> {
    let pair = payload
        .as_array()?
        .iter()
        .filter(|pair| {
            pair.get("chainId").and_then(Value::as_str) == Some(chain.market_network())
                && pair
                    .pointer("/baseToken/address")
                    .and_then(Value::as_str)
                    .is_some_and(|address| match chain {
                        Chain::Solana => address == token,
                        Chain::Robinhood | Chain::Bnb => address.eq_ignore_ascii_case(token),
                    })
                && positive(pair.get("priceUsd")).is_some()
                && pair
                    .get("pairAddress")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty())
        })
        .max_by(|a, b| {
            number(a.pointer("/liquidity/usd"))
                .unwrap_or(0.0)
                .total_cmp(&number(b.pointer("/liquidity/usd")).unwrap_or(0.0))
        })?;
    Some((
        MarketSnapshot {
            basis: Some(format!(
                "dexscreener:{}",
                pair.get("pairAddress")?.as_str()?
            )),
            name: pair
                .pointer("/baseToken/name")
                .and_then(Value::as_str)
                .map(str::to_string),
            symbol: pair
                .pointer("/baseToken/symbol")
                .and_then(Value::as_str)
                .map(str::to_string),
            price_usd: positive(pair.get("priceUsd")),
            liquidity_usd: number(pair.pointer("/liquidity/usd")),
            market_cap_usd: positive(pair.get("marketCap")),
            fdv_usd: positive(pair.get("fdv")),
            volume_h24_usd: number(pair.pointer("/volume/h24")),
            buys_h1: pair.pointer("/txns/h1/buys").and_then(Value::as_u64),
            sells_h1: pair.pointer("/txns/h1/sells").and_then(Value::as_u64),
            ..MarketSnapshot::default()
        },
        pair.get("pairAddress")?.as_str()?.to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    const TOKEN: &str = "7uNUAogctSAby1pZUAt2QmBpe2bradWeep78adZmpump";

    fn pair() -> Value {
        json!({"chainId":"solana", "pairAddress":"GUK8N22XBkFEguoHbb3unVLM5PYYbCydAHdysjtetA15",
            "baseToken":{"address":TOKEN,"name":"Todd","symbol":"todd"},
            "quoteToken":{"address":"So11111111111111111111111111111111111111112"},
            "priceUsd":"0.00004343", "liquidity":{"usd":17216.13}, "marketCap":41880,
            "volume":{"h24":5143.98}, "txns":{"h1":{"buys":2,"sells":2}}})
    }

    #[test]
    fn fallback_uses_exact_base_token_chain_and_deepest_pool() {
        let correct = pair();
        let mut shallow = correct.clone();
        shallow["liquidity"]["usd"] = json!(1);
        shallow["priceUsd"] = json!("0.1");
        let mut wrong = correct.clone();
        wrong["baseToken"]["address"] = json!("unrelated");
        wrong["liquidity"]["usd"] = json!(9999999);
        let mut wrong_chain = correct.clone();
        wrong_chain["chainId"] = json!("robinhood");
        let (snapshot, _) = fallback_snapshot(
            &json!([wrong, shallow, correct.clone(), wrong_chain]),
            Chain::Solana,
            TOKEN,
        )
        .unwrap();
        assert_eq!(snapshot.price_usd, Some(0.00004343));
        assert_eq!(snapshot.liquidity_usd, Some(17216.13));
        assert_eq!(snapshot.buys_h1, Some(2));
        assert!(snapshot.buyers_h1.is_none());
        assert!(snapshot.sellers_h1.is_none());
        assert!(fallback_snapshot(
            &json!([correct.clone()]),
            Chain::Solana,
            "So11111111111111111111111111111111111111112"
        )
        .is_none());
        assert!(fallback_snapshot(
            &json!([correct]),
            Chain::Solana,
            &TOKEN.to_ascii_lowercase()
        )
        .is_none());
        assert_ne!(
            asset_key(Chain::Solana, TOKEN),
            asset_key(Chain::Solana, &TOKEN.to_ascii_lowercase())
        );
        assert_eq!(
            asset_key(Chain::Robinhood, "0xAB"),
            asset_key(Chain::Robinhood, "0xab")
        );
    }

    #[test]
    fn bnb_fallback_requires_bsc_and_keeps_same_address_on_rh_separate() {
        let token = "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c";
        let mut bnb = pair();
        bnb["chainId"] = json!("bsc");
        bnb["baseToken"]["address"] = json!(token.to_ascii_uppercase());
        let mut rh = bnb.clone();
        rh["chainId"] = json!("robinhood");
        rh["liquidity"]["usd"] = json!(99999999);
        let (snapshot, _) =
            fallback_snapshot(&json!([bnb, rh.clone()]), Chain::Bnb, token).unwrap();
        assert_eq!(snapshot.liquidity_usd, Some(17216.13));
        assert!(fallback_snapshot(&json!([rh]), Chain::Bnb, token).is_none());
        assert_ne!(
            asset_key(Chain::Bnb, token),
            asset_key(Chain::Robinhood, token)
        );
    }

    async fn mock(
        primary_ok: bool,
        fallback_ok: Arc<AtomicBool>,
    ) -> (
        MarketClient,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
        let primary_count = Arc::new(AtomicUsize::new(0));
        let fallback_count = Arc::new(AtomicUsize::new(0));
        let pc = primary_count.clone();
        let primary = move || {
            let pc = pc.clone();
            async move {
                pc.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(40)).await;
                if primary_ok {
                    Json(json!({"data":{"attributes":{"name":"Todd","price_usd":"0.00004343"}}}))
                        .into_response()
                } else {
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "60")],
                        Json(json!({"status":{"error_code":429}})),
                    )
                        .into_response()
                }
            }
        };
        let fc = fallback_count.clone();
        let fallback = move || {
            let fc = fc.clone();
            let ok = fallback_ok.clone();
            async move {
                fc.fetch_add(1, Ordering::SeqCst);
                Json(if ok.load(Ordering::SeqCst) {
                    json!([pair()])
                } else {
                    json!([])
                })
            }
        };
        let app = Router::new()
            .route("/networks/solana/tokens/{token}", get(primary))
            .route("/token-pairs/v1/solana/{token}", get(fallback));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = Client::new();
        let gecko = GeckoClient::new(http.clone(), url.clone());
        (
            MarketClient::new(http, gecko, url),
            primary_count,
            fallback_count,
            task,
        )
    }

    #[tokio::test]
    async fn concurrent_rate_limited_reads_share_one_labelled_fallback_and_cooldown() {
        let (client, primary, fallback, task) = mock(false, Arc::new(AtomicBool::new(true))).await;
        let reads =
            futures::future::join_all((0..8).map(|_| client.snapshot(Chain::Solana, TOKEN))).await;
        assert_eq!(primary.load(Ordering::SeqCst), 1);
        assert_eq!(fallback.load(Ordering::SeqCst), 1);
        for read in reads {
            assert_eq!(read.snapshot.unwrap().price_usd, Some(0.00004343));
            assert!(!read.sources[0].ok && read.sources[0].detail.contains("rate-limited"));
            assert!(read.sources[1].ok && read.sources[1].source.contains("Dexscreener"));
        }
        // Optional metadata must observe the same provider cooldown, without HTTP.
        assert!(client.gecko.token_info(Chain::Solana, TOKEN).await.is_err());
        assert_eq!(primary.load(Ordering::SeqCst), 1);
        task.abort();
    }

    #[tokio::test]
    async fn healthy_gecko_stays_primary_and_concurrent_reads_are_coalesced() {
        let (client, primary, fallback, task) = mock(true, Arc::new(AtomicBool::new(true))).await;
        let reads =
            futures::future::join_all((0..8).map(|_| client.snapshot(Chain::Solana, TOKEN))).await;
        assert_eq!(primary.load(Ordering::SeqCst), 1);
        assert_eq!(fallback.load(Ordering::SeqCst), 0);
        assert!(reads
            .iter()
            .all(|r| r.sources.len() == 1 && r.sources[0].ok));
        task.abort();
    }

    #[tokio::test]
    async fn unavailable_sources_remain_unknown_and_a_later_retry_recovers() {
        let available = Arc::new(AtomicBool::new(false));
        let (client, primary, fallback, task) = mock(false, available.clone()).await;
        let failed = client.snapshot(Chain::Solana, TOKEN).await;
        assert!(failed.snapshot.is_none() && failed.sources.iter().all(|s| !s.ok));
        available.store(true, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let recovered = client.snapshot(Chain::Solana, TOKEN).await;
        assert!(recovered.snapshot.is_some());
        assert_eq!(primary.load(Ordering::SeqCst), 1);
        assert_eq!(fallback.load(Ordering::SeqCst), 2);
        task.abort();
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn exact_chain_and_base_identity_are_required_and_decimals_do_not_round_tiny_prices() {
        let address = "0x1111111111111111111111111111111111111111";
        let payload = json!([
            {"chainId":"ethereum","baseToken":{"address":address,"name":"Wrong chain"},"priceUsd":"999","liquidity":{"usd":1000000}},
            {"chainId":"bsc","baseToken":{"address":"0x2222222222222222222222222222222222222222"},"quoteToken":{"address":address},"priceUsd":"888","liquidity":{"usd":1000000}},
            {"chainId":"bsc","baseToken":{"address":address,"name":"Thin pool","symbol":"TEST"},"priceUsd":"1","liquidity":{"usd":1}},
            {"chainId":"bsc","baseToken":{"address":address,"name":"Synthetic test asset","symbol":"TEST"},"priceUsd":"0.0000000000527123456789","liquidity":{"usd":20}}
        ]);
        let received = batch_quotes(&payload, Chain::Bnb, &[address.into()], 123);
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].name.as_deref(), Some("Synthetic test asset"));
        assert_eq!(
            received[0].price_usd.unwrap().to_string(),
            "0.0000000000527123456789"
        );
        assert!(batch_quotes(&payload, Chain::Solana, &[address.into()], 123).is_empty());
    }
}
