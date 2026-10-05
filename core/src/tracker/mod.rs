mod accounting;
mod bnb;
mod budget;
mod evidence;
pub mod model;
mod providers;
mod store;
mod venues;

use crate::{config::Config, model::Chain, providers::gecko::GeckoClient};
use model::*;
use providers::Providers;
use serde_json::{json, Value};
use std::{collections::BTreeSet, sync::Arc};
use store::Store;

pub struct Tracker {
    store: Option<Arc<Store>>,
    providers: Option<Providers>,
    gecko: GeckoClient,
    native_prices: crate::providers::native_prices::NativePriceClient,
    error: Option<String>,
    interval: u64,
    current_interval: u64,
    cohort_limit: usize,
    record_limit: usize,
    public_nominations: bool,
    evidence_workers: Arc<tokio::sync::Semaphore>,
}

fn env_number(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(min, max)
}
fn secret(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn helius_keys(list: Option<String>, legacy: Option<String>) -> Vec<String> {
    let mut keys = Vec::new();
    for key in list
        .or(legacy)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(8)
    {
        if !keys.iter().any(|v| v == key) {
            keys.push(key.to_string());
        }
    }
    keys
}

fn balanced_candidates(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut groups: std::collections::BTreeMap<&str, std::collections::VecDeque<Candidate>> =
        std::collections::BTreeMap::new();
    for c in candidates {
        groups.entry(c.chain.key()).or_default().push_back(c);
    }
    let mut out = Vec::new();
    while groups.values().any(|g| !g.is_empty()) {
        for g in groups.values_mut() {
            if let Some(c) = g.pop_front() {
                out.push(c);
            }
        }
    }
    out
}

#[cfg(test)]
mod config_tests {
    use super::*;
    #[test]
    fn credential_lists_are_trimmed_deduplicated_and_do_not_change_budget() {
        assert_eq!(
            helius_keys(Some(" a, b, a, , c ".into()), Some("legacy".into())),
            vec!["a", "b", "c"]
        );
        assert_eq!(helius_keys(None, Some(" old ".into())), vec!["old"]);
        assert!(helius_keys(None, None).is_empty());
    }
    #[test]
    fn discovery_does_not_fill_the_cohort_before_a_chain_gets_a_turn() {
        let candidate = |chain, w: &str| Candidate {
            observed_tokens: Vec::new(),
            chain,
            wallet: w.into(),
            discovered_at: 0,
            sources: vec![],
        };
        let candidates = vec![
            candidate(Chain::Solana, "s1"),
            candidate(Chain::Solana, "s2"),
            candidate(Chain::Robinhood, "r1"),
            candidate(Chain::Bnb, "b1"),
        ];
        let first = balanced_candidates(candidates)
            .into_iter()
            .take(3)
            .map(|c| c.chain.key())
            .collect::<BTreeSet<_>>();
        assert_eq!(first, BTreeSet::from(["bnb", "robinhood", "solana"]));
    }

    #[tokio::test]
    async fn current_history_is_committed_when_the_history_work_allocation_is_exhausted() {
        use axum::{
            routing::{get, post},
            Json, Router,
        };
        async fn rpc(Json(request): Json<Value>) -> Json<Value> {
            if request["method"] == "eth_chainId" {
                Json(json!({"jsonrpc":"2.0","id":request["id"],"result":"0x1237"}))
            } else {
                Json(
                    json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"This test source does not provide transaction or balance bodies"}}),
                )
            }
        }
        async fn bnb_rpc(Json(request): Json<Value>) -> Json<Value> {
            assert_eq!(request["method"], "eth_chainId");
            Json(json!({"jsonrpc":"2.0","id":request["id"],"result":"0x38"}))
        }
        async fn index() -> Json<Value> {
            let evidence: Value = serde_json::from_str(include_str!(
                "../../tests/fixtures/tracker-rh-native-swap.json"
            ))
            .unwrap();
            Json(json!({"items":[{"hash":evidence["id"]}],"next_page_params":null}))
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/", post(rpc))
                    .route("/bnb", post(bnb_rpc))
                    .route("/addresses/{wallet}/{route}", get(index)),
            )
            .await
            .unwrap();
        });
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let config = Config {
            port: 0,
            gecko_api_host: url.clone(),
            dexscreener_api_host: url.clone(),
            solana_rpc_url: url.clone(),
            solana_fallback_rpc_url: url.clone(),
            robinhood_rpc_url: url.clone(),
            bnb_rpc_url: format!("{url}/bnb"),
            bnb_fallback_rpc_url: url.clone(),
            blockscout_api_url: url.clone(),
            blockscout_api_key: Some("synthetic-test-key".into()),
        };
        let store = Arc::new(Store::open(":memory:").unwrap());
        let evidence: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/tracker-rh-native-swap.json"
        ))
        .unwrap();
        let candidate = Candidate {
            chain: Chain::Robinhood,
            wallet: evidence["wallet"].as_str().unwrap().into(),
            discovered_at: now(),
            sources: vec![],
            observed_tokens: vec![],
        };
        store.nominate(candidate.clone(), 1).unwrap();
        let old = now() - 7200;
        store
            .save_page(
                &candidate,
                &[],
                &Coverage {
                    last_collected_at: Some(old),
                    last_state_checked_at: Some(old),
                    ..Default::default()
                },
                &Default::default(),
            )
            .unwrap();
        store
            .set_state(&format!("lane:history:{}", now() / DAY), "1900")
            .unwrap();
        store
            .set_state(&format!("requests:{}", now() / DAY), "1900")
            .unwrap();
        let providers = Providers {
            http: http.clone(),
            config,
            store: store.clone(),
            helius_keys: vec![],
            helius_credit_limit: 800000,
            fomo_key: None,
            rh_trace_url: url.clone(),
            bnb_trace_url: url.clone(),
            daily_limit: 2000,
            lane: Some(budget::Lane::Current),
        };
        let tracker = Tracker {
            store: Some(store.clone()),
            providers: Some(providers),
            gecko: GeckoClient::new(http.clone(), url),
            native_prices: crate::providers::native_prices::NativePriceClient::new(
                http,
                "http://127.0.0.1:1".into(),
            ),
            error: None,
            interval: 60,
            current_interval: 2400,
            cohort_limit: 1,
            record_limit: 10000,
            public_nominations: false,
            evidence_workers: Arc::new(tokio::sync::Semaphore::new(2)),
        };
        let started = now();
        tracker.tick().await.unwrap();
        let snapshot = store
            .snapshot(candidate.chain, &candidate.wallet)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.records.len(), 1);
        assert!(snapshot.coverage.last_collected_at.unwrap() >= started);
        assert_eq!(snapshot.coverage.last_state_checked_at, Some(old));
        let current_used = store.lane_used(budget::Lane::Current, now()).unwrap();
        assert_eq!(current_used, 7); // Four head routes plus bounded execution/state attempts.
        tracker.tick().await.unwrap();
        assert_eq!(
            store.lane_used(budget::Lane::Current, now()).unwrap(),
            current_used
        );
        assert_eq!(store.lane_used(budget::Lane::History, now()).unwrap(), 1900);
        assert!(!tracker
            .detail(candidate.chain, &candidate.wallet)
            .unwrap()
            .unwrap()
            .status
            .starts_with("qualified_"));
        let bnb_candidate = Candidate {
            chain: Chain::Bnb,
            wallet: "0xa2178d43b46152509c2efcebc5b9b77a50884b45".into(),
            discovered_at: now(),
            sources: vec![],
            observed_tokens: vec![],
        };
        store.nominate(bnb_candidate.clone(), 2).unwrap();
        store
            .save_page(
                &bnb_candidate,
                &[],
                &Coverage {
                    last_collected_at: Some(old),
                    ..Default::default()
                },
                &Default::default(),
            )
            .unwrap();
        tracker.collect_current(&bnb_candidate).await.unwrap();
        let bnb = store
            .snapshot(Chain::Bnb, &bnb_candidate.wallet)
            .unwrap()
            .unwrap();
        assert_eq!(bnb.coverage.last_collected_at, Some(old));
        assert!(!bnb.coverage.head_complete);
        assert!(bnb.records.is_empty());
        store
            .set_state(
                &format!("requests:{}", now() / DAY),
                &(2000 + store.lane_used(budget::Lane::Current, now()).unwrap()).to_string(),
            )
            .unwrap();
        let status = tracker.status();
        assert_eq!(status["collection_state"], "background_paused");
        for lane in status["request_allocations"].as_array().unwrap() {
            if lane["purpose"] == "current" {
                assert!(lane["available_now"].as_u64().unwrap() > 0);
                continue;
            }
            assert_eq!(lane["available_now"], 0);
            assert_eq!(lane["next_attempt_at"], status["budget_resets_at"]);
        }
        task.abort();
    }
    #[tokio::test]
    async fn current_solana_balances_replace_old_reconstruction_even_when_background_is_paused() {
        use axum::{routing::post, Json, Router};
        use rust_decimal::Decimal;
        async fn rpc(Json(request): Json<Value>) -> Json<Value> {
            let result = match request["method"].as_str().unwrap() {
                "getSignaturesForAddress" => json!([]),
                "getTokenAccountsByOwner" => {
                    let rows = if request["params"][1]["programId"]
                        == "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
                    {
                        json!([{"account":{"data":{"parsed":{"info":{"mint":"synthetic-token","tokenAmount":{"amount":"125","decimals":2}}}}}}])
                    } else {
                        json!([])
                    };
                    json!({"context":{"slot":123},"value":rows})
                }
                "getAccountInfo" => {
                    json!({"context":{"slot":123},"value":{"owner":"11111111111111111111111111111111"}})
                }
                other => panic!("Unexpected source method {other}"),
            };
            Json(json!({"jsonrpc":"2.0","id":1,"result":result}))
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/", post(rpc)))
                .await
                .unwrap();
        });
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut config = Config::from_env();
        config.solana_fallback_rpc_url = url.clone();
        let store = Arc::new(Store::open(":memory:").unwrap());
        let at = now();
        let candidate = Candidate {
            chain: Chain::Solana,
            wallet: "76CzYNKfkKqLuVANS1B43DBwvHtSLxqoYwZdTnmm3Z82".into(),
            discovered_at: at,
            sources: vec![],
            observed_tokens: vec![],
        };
        store.nominate(candidate.clone(), 1).unwrap();
        let record = Record {
            id: "synthetic-old-record".into(),
            raw: json!({"synthetic":true}),
            error: None,
            transaction: Some(Transaction {
                id: "synthetic-old-record".into(),
                timestamp: at - 100,
                block: 1,
                index: Some(0),
                finalized: true,
                succeeded: true,
                assets: vec![Delta {
                    asset: "synthetic-token".into(),
                    quantity: Decimal::from(9),
                }],
                fee_asset: "SOL".into(),
                fee_quantity: Some(Decimal::ZERO),
                movement_complete: true,
                swap_evidence: false,
                notes: vec![],
                counterparties: vec![],
            }),
        };
        store
            .save_page(
                &candidate,
                &[record],
                &Coverage::default(),
                &Default::default(),
            )
            .unwrap();
        store
            .save_token_quotes(
                Chain::Solana,
                &[crate::model::TokenQuote {
                    asset: "synthetic-token".into(),
                    price_usd: Some(Decimal::from(2)),
                    observed_at: at,
                    source: "Synthetic test mark".into(),
                    ..Default::default()
                }],
            )
            .unwrap();
        store
            .set_state(&format!("requests:{}", at / DAY), "2000")
            .unwrap();
        let providers = Providers {
            http: http.clone(),
            config,
            store: store.clone(),
            helius_keys: vec![],
            helius_credit_limit: 800000,
            fomo_key: None,
            rh_trace_url: url.clone(),
            bnb_trace_url: url.clone(),
            daily_limit: 2000,
            lane: Some(budget::Lane::Current),
        };
        let tracker = Tracker {
            store: Some(store.clone()),
            providers: Some(providers),
            gecko: GeckoClient::new(http.clone(), url.clone()),
            native_prices: crate::providers::native_prices::NativePriceClient::new(http, url),
            error: None,
            interval: 60,
            current_interval: 360,
            cohort_limit: 1,
            record_limit: 10000,
            public_nominations: false,
            evidence_workers: Arc::new(tokio::sync::Semaphore::new(2)),
        };
        tracker.tick().await.unwrap();
        let detail = tracker
            .detail(Chain::Solana, &candidate.wallet)
            .unwrap()
            .unwrap();
        assert_eq!(detail.positions[0].quantity, Decimal::from(9));
        assert_eq!(
            detail.positions[0].valuation.as_ref().unwrap().quantity,
            Decimal::new(125, 2)
        );
        assert_eq!(
            detail.positions[0].market_value_usd,
            Some(Decimal::new(25, 1))
        );
        assert!(detail.positions[0]
            .valuation
            .as_ref()
            .unwrap()
            .quantity_block
            .as_ref()
            .unwrap()
            .contains("123"));
        assert!(!detail.status.starts_with("qualified_"));
        assert_eq!(tracker.status()["collection_state"], "background_paused");
        assert_eq!(
            store
                .state(&format!("requests:{}", at / DAY))
                .unwrap()
                .as_deref(),
            Some("2004")
        );
        server.abort();
    }
}

fn demote_stale(analysis: &mut Analysis) {
    let state_stale = analysis
        .coverage
        .last_state_checked_at
        .is_none_or(|t| now().saturating_sub(t) > 3600);
    if state_stale {
        if analysis.status.starts_with("qualified_") {
            analysis.status = "incomplete".into();
        }
        for window in &mut analysis.windows {
            window.qualified = false;
            for gate in &mut window.gates {
                if gate.name == "Recent and fresh" {
                    gate.passed = false;
                }
            }
        }
    }
    if analysis
        .coverage
        .last_collected_at
        .is_some_and(|t| now().saturating_sub(t) > 3600)
    {
        analysis.status = "stale".into();
        for window in &mut analysis.windows {
            window.qualified = false;
            for gate in &mut window.gates {
                if gate.name == "Recent and fresh" {
                    gate.passed = false;
                }
            }
        }
    }
}

fn ranking_key(
    analysis: &Analysis,
) -> (
    Option<rust_decimal::Decimal>,
    Option<rust_decimal::Decimal>,
    usize,
) {
    let windows: Vec<_> = match analysis.status.as_str() {
        "qualified_60d" => analysis.windows.iter().collect(),
        "qualified_30d" => analysis.windows.last().into_iter().collect(),
        _ => vec![],
    };
    (
        windows.iter().filter_map(|w| w.profit_factor).min(),
        windows
            .iter()
            .filter_map(|w| w.profit_without_largest_usd)
            .min(),
        windows.iter().map(|w| w.episodes).min().unwrap_or(0),
    )
}

impl Tracker {
    pub fn new(http: reqwest::Client, config: Config, gecko: GeckoClient) -> Arc<Self> {
        let (store, error) = match secret("WATER_TRACKER_DB_PATH") {
            Some(path) => match Store::open(&path) {
                Ok(store) => (Some(Arc::new(store)), None),
                Err(e) => (None, Some(e)),
            },
            None => (
                None,
                Some(
                    "Wallet collection is paused because evidence storage is not configured."
                        .into(),
                ),
            ),
        };
        let native_prices = crate::providers::native_prices::NativePriceClient::new(
            http.clone(),
            "https://api.kraken.com".into(),
        );
        let providers = store.as_ref().map(|store| Providers {
            http,
            config,
            store: store.clone(),
            helius_keys: helius_keys(secret("HELIUS_API_KEYS"), secret("HELIUS_API_KEY")),
            helius_credit_limit: env_number("WATER_TRACKER_HELIUS_CREDITS_31D", 800000, 10, 1000000)
                as u64,
            fomo_key: secret("FOMO_DISCOVERY_API_KEY"),
            rh_trace_url: std::env::var("ROBINHOOD_TRACE_RPC_URL")
                .unwrap_or_else(|_| "https://robinhood.drpc.org".into()),
            bnb_trace_url: std::env::var("BNB_TRACE_RPC_URL")
                .unwrap_or_else(|_| "https://bsc.drpc.org".into()),
            daily_limit: env_number("WATER_TRACKER_DAILY_REQUESTS", 2000, 10, 2500) as u64,
            lane: Some(budget::Lane::Current),
        });
        Arc::new(Self {
            evidence_workers: Arc::new(tokio::sync::Semaphore::new(2)),
            store,
            providers,
            gecko,
            native_prices,
            error,
            interval: env_number("WATER_TRACKER_INTERVAL_SECONDS", 60, 30, 3600) as u64,
            current_interval: env_number("WATER_TRACKER_REFRESH_SECONDS", 360, 300, 3000) as u64,
            cohort_limit: env_number("WATER_TRACKER_COHORT_LIMIT", 12, 2, 48),
            record_limit: env_number("WATER_TRACKER_RECORD_LIMIT", 10000, 100, 50000),
            public_nominations: std::env::var("WATER_TRACKER_ALLOW_PUBLIC_NOMINATIONS").as_deref()
                == Ok("true"),
        })
    }

    pub fn start(self: &Arc<Self>) {
        if self.store.is_none() {
            return;
        }
        let market_tracker = self.clone();
        tokio::spawn(async move {
            loop {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(45),
                    market_tracker.enrich_tick(),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(worker = "market", error = %error, "Wallet enrichment failed");
                        let _ = market_tracker
                            .store
                            .as_ref()
                            .unwrap()
                            .set_state("market_error", &error);
                    }
                    Err(_) => {
                        tracing::warn!(worker = "market", "Wallet enrichment exceeded its 45 second time budget");
                        let _=market_tracker.store.as_ref().unwrap().set_state("market_error","Market enrichment exceeded its work time budget; received marks are retained.");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
        });
        let tracker = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = tracker.tick().await {
                    if let Some(store) = &tracker.store {
                        let _ = store.set_state("collector_error", &error);
                    }
                    tracing::warn!("Wallet collector: {}", error);
                }
                tokio::time::sleep(std::time::Duration::from_secs(tracker.interval)).await;
            }
        });
    }

    pub fn status(&self) -> Value {
        let Some(store) = &self.store else {
            return json!({"enabled":false,"detail":self.error,"wallets":[],"policy":POLICY});
        };
        let providers = self.providers.as_ref().unwrap();
        let timestamp = now();
        let resets_at = (timestamp / DAY + 1) * DAY;
        let used = store
            .state(&format!("requests:{}", timestamp / DAY))
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        let current_used = store
            .lane_used(budget::Lane::Current, timestamp)
            .unwrap_or(0);
        let background_used = used.saturating_sub(current_used);
        let background_paused = background_used >= providers.daily_limit;
        let current_paused = current_used >= store.current_limit;
        let lanes:Vec<_>=[budget::Lane::Current,budget::Lane::History,budget::Lane::Discovery].into_iter().map(|lane|{
            let current=matches!(lane,budget::Lane::Current);
            let daily=if current {store.current_limit}else{providers.daily_limit};
            let count=store.lane_used(lane,timestamp).unwrap_or(0);
            let paused=if current {current_paused}else{background_paused};
            json!({"purpose":lane.key(),"used":count,"limit":lane.limit(daily),"available_now":if paused {0}else{lane.allowance(timestamp,daily).saturating_sub(count)},"next_attempt_at":if paused {resets_at}else{lane.next_attempt(timestamp,daily,count)}})
        }).collect();
        let credit_paused =
            store.helius_credits(timestamp).unwrap_or(0) >= providers.helius_credit_limit;
        json!({"collection_state":if current_paused {"budget_paused"}else if background_paused {"background_paused"}else{"scheduled"},"budget_resets_at":resets_at,"current_refresh_seconds":self.effective_interval(),"request_allocations":lanes,"background_requests_today":background_used,"current_request_limit":store.current_limit,"history_detail":store.state("history_error").ok().flatten(),"market_detail":store.state("market_error").ok().flatten(),"helius_budget_paused":credit_paused,"enabled":true,"nomination_enabled":self.public_nominations,"detail":store.state("collector_error").ok().flatten(),"policy":POLICY,"interval_seconds":self.interval,"cohort_limit":self.cohort_limit,"requests_today":used,"daily_request_limit":providers.daily_limit,"solana_indexed_access":!providers.helius_keys.is_empty(),"rh_indexed_access":providers.config.blockscout_api_key.is_some(),"helius_key_count":providers.helius_keys.len(),"helius_credits_reserved_31d":store.helius_credits(timestamp).unwrap_or(0),"helius_credit_limit_31d":providers.helius_credit_limit,"bnb_history_scope":"Public token-transfer discovery; complete wallet/native-history coverage is unproved.","fomo_discovery_access":providers.fomo_key.is_some(),"last_discovery_at":store.state("discovery_time").ok().flatten().and_then(|v|v.parse::<u64>().ok()),"discovery_notes":store.state("discovery_notes").ok().flatten().and_then(|v|serde_json::from_str::<Value>(&v).ok()),"storage_configured":!store.path.is_empty(),"storage_durability":"Requires a persistent deployment volume; path configuration does not prove durability."})
    }

    fn effective_interval(&self) -> u64 {
        let Some(store) = &self.store else {
            return self.current_interval;
        };
        // Worst-case SOL head = 10 indexed + 1 standard RPC credit. Scale the
        // cadence with cohort size instead of exhausting a free monthly plan.
        let analyses = store.analyses().unwrap_or_default();
        let count = store.wallet_count().unwrap_or(self.cohort_limit) as u64;
        let sol = analyses
            .iter()
            .filter(|a| matches!(a.candidate.chain, Chain::Solana))
            .count() as u64;
        let evm = count.saturating_sub(sol);
        let head_http = sol * 5 + evm * 28;
        let credits = self.providers.as_ref().unwrap().helius_credit_limit / 32 * 80 / 100;
        let at = now();
        let spent = store.lane_used(budget::Lane::Current, at).unwrap_or(0);
        let checks = store
            .state(&format!("current-checks:{}", at / DAY))
            .ok()
            .flatten()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let measured = if checks > 0 {
            (spent * count * 11).div_ceil(checks * 10)
        } else {
            head_http
        };
        let remaining = store.current_limit.saturating_sub(spent);
        let paced = if remaining > 0 {
            (head_http.max(measured) * (DAY - at % DAY)).div_ceil(remaining)
        } else {
            3600
        };
        self.current_interval
            .max((sol * 14 * DAY).div_ceil(credits.max(1)))
            .max((head_http * DAY).div_ceil(store.current_limit.max(1)))
            .max(paced)
            .min(3600)
    }

    pub fn list(&self) -> Result<Value, String> {
        let Some(store) = &self.store else {
            return Ok(json!({"status":self.status(),"wallets":[]}));
        };
        let mut analyses = store.analyses()?;
        for analysis in &mut analyses {
            demote_stale(analysis);
        }
        analyses.sort_by(|a, b| {
            let tier = |s: &str| match s {
                "qualified_60d" => 0,
                "qualified_30d" => 1,
                "observed" => 2,
                _ => 3,
            };
            tier(&a.status)
                .cmp(&tier(&b.status))
                .then_with(|| ranking_key(b).cmp(&ranking_key(a)))
                .then_with(|| a.candidate.chain.key().cmp(b.candidate.chain.key()))
                .then_with(|| a.candidate.wallet.cmp(&b.candidate.wallet))
        });
        let summaries: Vec<_> = analyses
            .into_iter()
            .map(|mut a| {
                a.activity.truncate(20);
                let positions_count = a.positions.len();
                let mut value = serde_json::to_value(a).unwrap();
                let fields = value.as_object_mut().unwrap();
                fields.remove("positions");
                fields.insert("positions_count".into(), json!(positions_count));
                fields.remove("notes");
                value
            })
            .collect();
        Ok(
            json!({"status":self.status(),"scope":"ranking_summary; full positions and notes are in wallet detail","wallets":summaries}),
        )
    }

    pub fn detail(&self, chain: Chain, wallet: &str) -> Result<Option<Analysis>, String> {
        let _permit = self.evidence_permit()?;
        let wallet = wallet_key(chain, wallet)?;
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let Some(snapshot) = store.snapshot(chain, &wallet)? else {
            return Ok(None);
        };
        let end = snapshot.coverage.last_collected_at.unwrap_or_else(now);
        let mut analysis = accounting::analyze(&snapshot, end.saturating_add(1));
        evidence::enrich(&mut analysis, &snapshot, store.token_quotes(chain)?, now());
        analysis.activity.truncate(100);
        demote_stale(&mut analysis);
        Ok(Some(analysis))
    }

    pub fn activity(&self, request: ActivityRequest) -> Result<Option<Value>, String> {
        let _permit = self.evidence_permit()?;
        let wallet = wallet_key(request.chain, &request.wallet)?;
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let Some((snapshot, revision)) = store.activity_snapshot(
            request.chain,
            &wallet,
            request.transaction.as_deref(),
        )? else {
            return Ok(None);
        };
        Ok(Some(evidence::activity_page_with_revision(
            &snapshot,
            &store.token_quotes(request.chain)?,
            &request,
            Some(revision),
        )?))
    }

    pub fn nominate(&self, request: WalletRequest) -> Result<Analysis, String> {
        if !self.public_nominations {
            return Err(
                "Public nominations are disabled to protect the shared free collection budget."
                    .into(),
            );
        }
        let _permit = self.evidence_permit()?;
        let wallet = wallet_key(request.chain, &request.wallet)?;
        let store = self
            .store
            .as_ref()
            .ok_or("Wallet collection is paused until evidence storage is configured.")?;
        let candidate=Candidate{observed_tokens:Vec::new(),chain:request.chain,wallet:wallet.clone(),discovered_at:now(),sources:vec![Source{name:"Added for research".into(),observed_at:now(),detail:"A nomination requests evidence collection; it does not verify ownership or profitability.".into(),profile:None}]};
        store.nominate(candidate, self.cohort_limit)?;
        let analysis = accounting::analyze(
            &store
                .snapshot(request.chain, &wallet)?
                .ok_or("Wallet nomination was not saved.")?,
            now(),
        );
        store.save_analysis(&analysis)?;
        Ok(analysis)
    }

    pub fn token_overlap(&self, chain: Chain, token: &str) -> Result<Value, String> {
        let token = wallet_key(chain, token)?;
        let Some(store) = &self.store else {
            return Ok(json!({"wallets":[]}));
        };
        let mut wallets = Vec::new();
        for mut analysis in store.analyses()? {
            demote_stale(&mut analysis);
            if analysis.candidate.chain.key() != chain.key()
                || !analysis.status.starts_with("qualified_")
                || analysis
                    .coverage
                    .last_collected_at
                    .is_none_or(|t| now().saturating_sub(t) > 3600)
            {
                continue;
            }
            if let Some(position) = analysis
                .positions
                .iter()
                .find(|p| p.asset == token && p.quantity > rust_decimal::Decimal::ZERO)
            {
                wallets.push(json!({"wallet":analysis.candidate.wallet,"status":analysis.status,"position":position,"analyzed_at":analysis.analyzed_at}));
            }
        }
        Ok(
            json!({"chain":chain,"token":token,"wallets":wallets,"scope":"Fresh qualifying wallets in Water's bounded collected cohort. Shared holdings do not prove independent ownership."}),
        )
    }

    async fn snapshot(&self, chain: Chain, wallet: &str) -> Result<Snapshot, String> {
        let store = self.store.clone().ok_or("Tracker is not configured.")?;
        let wallet = wallet.to_string();
        tokio::task::spawn_blocking(move || {
            store
                .snapshot(chain, &wallet)?
                .ok_or("Scheduled wallet disappeared.".into())
        })
        .await
        .map_err(|_| "Wallet evidence worker failed.")?
    }

    async fn update_analysis(&self, chain: Chain, wallet: &str, end: u64) -> Result<(), String> {
        let permit = self.evidence_workers.clone().acquire_owned().await
            .map_err(|_| "Wallet accounting is unavailable.")?;
        let store = self.store.clone().unwrap();
        let wallet = wallet.to_owned();
        tokio::task::spawn_blocking(move || {
            // A timed-out caller cannot cancel a started blocking task. Its
            // permit stays here until the database read and accounting finish.
            let _permit = permit;
            let snapshot = store.snapshot(chain, &wallet)?
                .ok_or("Scheduled wallet disappeared.")?;
            let end = snapshot.coverage.last_collected_at.unwrap_or(end);
            let mut analysis = accounting::analyze(&snapshot, end.saturating_add(1));
            analysis.activity.truncate(100);
            store.save_analysis(&analysis)
        })
        .await
        .map_err(|_| "Wallet accounting worker failed.")?
    }

    fn evidence_permit(&self) -> Result<tokio::sync::OwnedSemaphorePermit, String> {
        self.evidence_workers.clone().try_acquire_owned()
            .map_err(|_| "Wallet evidence is busy; retry shortly.".into())
    }

    async fn tick(&self) -> Result<(), String> {
        let store = self.store.as_ref().ok_or("Tracker is not configured.")?;
        let base = self.providers.as_ref().unwrap();
        let used = store
            .state(&format!("requests:{}", now() / DAY))?
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        // Current work owns a separate allocation, including during a legacy
        // background-budget pause. Different wallets may be checked together;
        // history runs afterwards to avoid concurrent coverage/cursor writes.
        if store.lane_used(budget::Lane::Current, now())? < store.current_limit {
            let mut candidates = Vec::new();
            for _ in 0..4 {
                if let Some(c) = store.next_wallet(now(), self.effective_interval())? {
                    candidates.push(c);
                } else {
                    break;
                }
            }
            if !candidates.is_empty() {
                let results=futures::future::join_all(candidates.iter().map(|c|async {
                    let started = std::time::Instant::now();
                    tracing::info!(chain = c.chain.key(), wallet = %c.wallet, "Current wallet collection started");
                    let result = tokio::time::timeout(std::time::Duration::from_secs(30),self.collect_current(c)).await
                        .unwrap_or_else(|_| Err("Current wallet check exceeded its collection time budget; saved evidence is retained.".to_string()));
                    if let Err(error) = &result {
                        tracing::warn!(chain = c.chain.key(), wallet = %c.wallet, elapsed_ms = started.elapsed().as_millis() as u64, error = %error, "Current wallet collection failed");
                    } else {
                        tracing::info!(chain = c.chain.key(), wallet = %c.wallet, elapsed_ms = started.elapsed().as_millis() as u64, "Current wallet collection finished");
                    }
                    result
                })).await;
                let key = format!("current-checks:{}", now() / DAY);
                let previous = store
                    .state(&key)?
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                store.set_state(&key, &(previous + candidates.len() as u64).to_string())?;
                let errors: Vec<_> = results.into_iter().filter_map(Result::err).collect();
                store.set_state("collector_error", &errors.join(" "))?;
                return Ok(());
            }
        }
        let current_used = store.lane_used(budget::Lane::Current, now())?;
        if used.saturating_sub(current_used) >= base.daily_limit {
            return Ok(());
        }
        if store.wallet_count()? < self.cohort_limit
            && store
                .state("discovery_time")?
                .and_then(|s| s.parse::<u64>().ok())
                .is_none_or(|t| now().saturating_sub(t) > 6 * 3600)
            && store.lane_used(budget::Lane::Discovery, now())?
                < budget::Lane::Discovery.limit(base.daily_limit)
        {
            let mut providers = base.clone();
            providers.lane = Some(budget::Lane::Discovery);
            store.set_state("discovery_time", &now().to_string())?;
            let Ok((candidates, notes)) = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                providers.discover(self.cohort_limit),
            )
            .await
            else {
                store.set_state("discovery_notes", &serde_json::to_string(&vec![
                    "Discovery exceeded its work time budget; saved candidates are retained and source coverage remains unverified."
                ]).unwrap())?;
                return Ok(());
            };
            for candidate in balanced_candidates(candidates) {
                let chain = candidate.chain;
                let wallet = candidate.wallet.clone();
                if store.nominate(candidate, self.cohort_limit).is_ok() {
                    self.update_analysis(chain, &wallet, now()).await?;
                }
            }
            store.set_state("discovery_notes", &serde_json::to_string(&notes).unwrap())?;
            return Ok(());
        }
        let used = store.lane_used(budget::Lane::History, now())?;
        if budget::Lane::History
            .allowance(now(), base.daily_limit)
            .saturating_sub(used)
            < budget::Lane::History.minimum_batch(base.daily_limit)
        {
            return Ok(());
        }
        if let Some(candidate) = store.next_history_wallet(now())? {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                self.collect_history(&candidate),
            )
            .await;
            match result {
                Ok(Ok(())) => {
                    store.set_state("history_error", "")?;
                }
                Ok(Err(error)) => {
                    tracing::warn!(chain = candidate.chain.key(), wallet = %candidate.wallet, error = %error, "Wallet history collection failed");
                    store.set_state("history_error", &error)?;
                }
                Err(_) => {
                    tracing::warn!(chain = candidate.chain.key(), wallet = %candidate.wallet, "Wallet history collection exceeded its 30 second time budget");
                    store.set_state("history_error","History pass exceeded its work time budget; committed records and cursors are retained.")?;
                }
            }
        }
        Ok(())
    }

    async fn collect_current(&self, candidate: &Candidate) -> Result<(), String> {
        let store = self.store.as_ref().unwrap();
        let providers = self.providers.as_ref().unwrap();
        let snapshot = self.snapshot(candidate.chain, &candidate.wallet).await?;
        if snapshot.records.len() >= self.record_limit {
            let mut coverage = snapshot.coverage.clone();
            coverage.history_complete = false;
            coverage.notes=vec!["Wallet history reached its record budget. Qualification is paused; no missing history is treated as complete.".into()];
            store.save_page(&candidate, &[], &coverage, &snapshot.balances)?;
            self.update_analysis(candidate.chain, &candidate.wallet, now())
                .await?;
            return Ok(());
        }
        let known: BTreeSet<_> = snapshot.records.iter().map(|r| r.id.clone()).collect();
        let mut coverage = snapshot.coverage.clone();
        let mut records = Vec::new();
        let head = providers.page(&candidate, None).await?;
        let overlap = if matches!(candidate.chain, Chain::Bnb) {
            providers::range_covered(&head, &coverage.public_scan_ranges)
        } else {
            providers::head_covered(&head, &known)
        };
        coverage.provider = if matches!(candidate.chain, Chain::Solana) {
            if head.indexed {
                "Helius indexed Solana history"
            } else {
                "Public Solana signature history"
            }
        } else if matches!(candidate.chain, Chain::Bnb) {
            "BNB public token-transfer references and direct RPC/trace evidence"
        } else {
            "RH Blockscout and direct RPC/trace evidence"
        }
        .into();
        coverage.pages += 1;
        coverage.notes = head.notes.clone();
        if !coverage.backfill_started {
            coverage.cursor = head.cursor.clone();
            coverage.backfill_started = true;
            coverage.backfill_done = head.exhausted;
            coverage.head_complete = true;
        } else if !overlap && !head.exhausted {
            coverage.head_complete = false;
            if let Some(cursor) = head.cursor.clone() {
                if coverage.head_cursor.is_none() {
                    coverage.head_cursor = Some(cursor);
                } else if coverage.head_cursor.as_ref() != Some(&cursor)
                    && !coverage.head_cursors.contains(&cursor)
                {
                    coverage.head_cursors.push(cursor);
                }
            }
        }
        providers::add_scan_range(&mut coverage.public_scan_ranges, head.scan_range);
        let received = !matches!(candidate.chain, Chain::Bnb) || head.scan_range.is_some();
        records.extend(head.records);
        // Bound a page's references without silently advancing beyond omitted
        // evidence. The cursor is saved only if every reference fits storage.
        let new = records
            .iter()
            .filter(|r| !known.contains(&r.id))
            .map(|r| &r.id)
            .collect::<BTreeSet<_>>()
            .len();
        if snapshot.records.len() + new > self.record_limit {
            let mut previous = snapshot.coverage.clone();
            previous.history_complete = false;
            previous.notes.push(
                "A new page exceeds the record budget. Its continuation was not advanced.".into(),
            );
            store.save_page(&candidate, &[], &previous, &snapshot.balances)?;
            self.update_analysis(candidate.chain, &candidate.wallet, now())
                .await?;
            return Err("The next history page exceeds the wallet record budget; continuation was not advanced.".into());
        }
        if new > 0 {
            coverage.history_complete = false;
        }
        if received {
            coverage.last_collected_at = Some(now());
        } else {
            coverage.head_complete = false;
        }
        store.save_page(&candidate, &records, &coverage, &snapshot.balances)?;
        self.update_analysis(candidate.chain, &candidate.wallet, now())
            .await?;
        if received {
            self.current_details(candidate).await?;
        }
        store.set_state("collector_error", "")?;
        Ok(())
    }

    async fn current_details(&self, candidate: &Candidate) -> Result<(), String> {
        let store = self.store.as_ref().unwrap();
        let providers = self.providers.as_ref().unwrap();
        let mut snapshot = self.snapshot(candidate.chain, &candidate.wallet).await?;
        // EVM indexes return references, not full executions. Resolve a recent
        // reference on the current allocation even when backfill is paused.
        if !matches!(candidate.chain, Chain::Solana) {
            let cutoff = now().saturating_sub(DAY);
            let mut pending: Vec<_> = snapshot
                .records
                .iter()
                .filter(|r| r.transaction.is_none())
                .filter(|r| {
                    // Block-only references may have no timestamp until fetched.
                    providers::record_timestamp(r).is_none_or(|t| t >= cutoff)
                })
                .collect();
            pending.sort_by_key(|r| std::cmp::Reverse(providers::record_priority(r)));
            if let Some(record) = pending.into_iter().find(|r| {
                store
                    .state(&format!(
                        "current-retry:{}:{}:{}",
                        candidate.chain.key(),
                        candidate.wallet,
                        r.id
                    ))
                    .ok()
                    .flatten()
                    .and_then(|s| s.parse::<u64>().ok())
                    .is_none_or(|t| t <= now())
            }) {
                let mut failed = store.record(candidate.chain, &candidate.wallet, &record.id)?
                    .ok_or("Saved transaction disappeared.")?;
                match providers.fetch_record(candidate, record).await {
                    Ok(received) => store.save_page(
                        candidate,
                        &[received],
                        &snapshot.coverage,
                        &snapshot.balances,
                    )?,
                    Err(error) => {
                        failed.error = Some(error);
                        store.save_page(
                            candidate,
                            &[failed],
                            &snapshot.coverage,
                            &snapshot.balances,
                        )?;
                    }
                }
                store.set_state(
                    &format!(
                        "current-retry:{}:{}:{}",
                        candidate.chain.key(),
                        candidate.wallet,
                        record.id
                    ),
                    &(now() + 900).to_string(),
                )?;
                self.update_analysis(candidate.chain, &candidate.wallet, now())
                    .await?;
                snapshot = self.snapshot(candidate.chain, &candidate.wallet).await?;
            }
        }
        let state_key = format!(
            "current-state:{}:{}",
            candidate.chain.key(),
            candidate.wallet
        );
        if store
            .state(&state_key)?
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|t| now() < t)
        {
            return Ok(());
        }
        store.set_state(&state_key, &(now() + 1800).to_string())?;
        let mut assets: BTreeSet<_> = snapshot
            .records
            .iter()
            .filter_map(|r| r.transaction.as_ref())
            .flat_map(|t| t.assets.iter().map(|d| d.asset.clone()))
            .filter(|a| !quote(candidate.chain, a))
            .collect();
        assets.extend(candidate.observed_tokens.iter().cloned());
        if !matches!(candidate.chain, Chain::Solana) {
            // Bounded, oldest balance first. Full-wallet reconciliation stays
            // separate; each valuation carries this asset's actual read time.
            let mut sorted: Vec<_> = assets.into_iter().collect();
            sorted.sort_by_key(|a| {
                snapshot
                    .coverage
                    .balance_observations
                    .get(a)
                    .map(|o| o.observed_at)
                    .unwrap_or(0)
            });
            assets = sorted.into_iter().take(2).collect();
        }
        let mut coverage = snapshot.coverage.clone();
        let mut balances = snapshot.balances.clone();
        match providers.balance_read(candidate, &assets).await {
            Ok((received, block)) => {
                let at = now();
                coverage.state_error = None;
                let full = matches!(candidate.chain, Chain::Solana);
                if full {
                    let mut known = assets.clone();
                    known.extend(balances.keys().cloned());
                    known.extend(received.keys().cloned());
                    for asset in known {
                        coverage.balance_observations.insert(
                            asset.clone(),
                            BalanceObservation {
                                quantity: received.get(&asset).copied().unwrap_or_default(),
                                observed_at: at,
                                block: block.clone(),
                                source: "Solana finalized token-account RPC".into(),
                            },
                        );
                    }
                    balances = received;
                    coverage.balances_observed_at = Some(at);
                } else {
                    for (asset, quantity) in received {
                        balances.insert(asset.clone(), quantity);
                        coverage.balance_observations.insert(
                            asset,
                            BalanceObservation {
                                quantity,
                                observed_at: at,
                                block: block.clone(),
                                source: format!(
                                    "{} finalized balanceOf RPC",
                                    candidate.chain.label()
                                ),
                            },
                        );
                    }
                }
                // Only a full state pass can refresh qualification's state clock.
                if full {
                    coverage.balances_reconciled = true;
                    let paid = snapshot
                        .records
                        .iter()
                        .filter_map(|r| r.transaction.as_ref())
                        .any(|t| {
                            t.fee_quantity
                                .is_some_and(|q| q > rust_decimal::Decimal::ZERO)
                        });
                    if let Ok(verified) = providers.execution_account(candidate, paid).await {
                        coverage.execution_account_verified = verified;
                        coverage.last_state_checked_at = Some(at);
                    }
                }
                store.save_page(candidate, &[], &coverage, &balances)?;
                self.update_analysis(candidate.chain, &candidate.wallet, now())
                    .await?;
            }
            Err(error) => {
                coverage.state_error = Some(error);
                store.save_page(candidate, &[], &coverage, &balances)?;
                self.update_analysis(candidate.chain, &candidate.wallet, now())
                    .await?;
            }
        }
        Ok(())
    }

    async fn collect_history(&self, candidate: &Candidate) -> Result<(), String> {
        let store = self.store.as_ref().unwrap();
        let mut providers = self.providers.as_ref().unwrap().clone();
        providers.lane = Some(budget::Lane::History);
        let snapshot = self.snapshot(candidate.chain, &candidate.wallet).await?;
        if snapshot.records.len() >= self.record_limit {
            return Ok(());
        }
        let known: BTreeSet<_> = snapshot.records.iter().map(|r| r.id.clone()).collect();
        let mut coverage = snapshot.coverage.clone();
        let mut records = Vec::new();
        if let Some(cursor) = coverage.head_cursor.clone() {
            let page = providers.page(&candidate, Some(&cursor)).await?;
            let caught_up = page.exhausted
                || if matches!(candidate.chain, Chain::Bnb) {
                    providers::range_covered(&page, &snapshot.coverage.public_scan_ranges)
                } else {
                    providers::head_covered(&page, &known)
                };
            coverage.head_cursor = if caught_up {
                if coverage.head_cursors.is_empty() {
                    None
                } else {
                    Some(coverage.head_cursors.remove(0))
                }
            } else {
                page.cursor
            };
            coverage.head_complete = coverage.head_cursor.is_none();
            coverage.pages += 1;
            providers::add_scan_range(&mut coverage.public_scan_ranges, page.scan_range);
            records.extend(page.records);
        } else if !coverage.backfill_done {
            if let Some(cursor) = coverage.cursor.clone() {
                let page = providers.page(&candidate, Some(&cursor)).await?;
                coverage.cursor = page.cursor;
                coverage.backfill_done = page.exhausted;
                coverage.pages += 1;
                providers::add_scan_range(&mut coverage.public_scan_ranges, page.scan_range);
                records.extend(page.records);
            }
        }
        // Bound a page's references without silently advancing beyond omitted
        // evidence. The cursor is saved only if every reference fits storage.
        let new = records
            .iter()
            .filter(|r| !known.contains(&r.id))
            .map(|r| &r.id)
            .collect::<BTreeSet<_>>()
            .len();
        if snapshot.records.len() + new > self.record_limit {
            let mut previous = snapshot.coverage.clone();
            previous.history_complete = false;
            previous.notes.push(
                "A new page exceeds the record budget. Its continuation was not advanced.".into(),
            );
            store.save_page(&candidate, &[], &previous, &snapshot.balances)?;
            self.update_analysis(candidate.chain, &candidate.wallet, now())
                .await?;
            return Err("The next history page exceeds the wallet record budget; continuation was not advanced.".into());
        }
        if new > 0 {
            coverage.history_complete = false;
        }
        store.save_page(&candidate, &records, &coverage, &snapshot.balances)?;
        self.update_analysis(candidate.chain, &candidate.wallet, now())
            .await?;
        let refreshed = self.snapshot(candidate.chain, &candidate.wallet).await?;
        let mut pending: Vec<_> = refreshed
            .records
            .iter()
            .filter(|r| {
                r.transaction
                    .as_ref()
                    .is_none_or(|t| !t.finalized || !t.movement_complete)
            })
            .filter(|r| {
                let key = format!(
                    "retry:{}:{}:{}",
                    candidate.chain.key(),
                    candidate.wallet,
                    r.id
                );
                store
                    .state(&key)
                    .ok()
                    .flatten()
                    .and_then(|v| v.parse::<u64>().ok())
                    .is_none_or(|t| now() >= t)
            })
            .cloned()
            .collect();
        pending.sort_by_key(|r| std::cmp::Reverse(providers::record_priority(r)));
        pending.truncate(3);
        let mut fetched = Vec::new();
        for reference in pending {
            let mut record = store.record(candidate.chain, &candidate.wallet, &reference.id)?
                .ok_or("Saved transaction disappeared.")?;
            match providers.fetch_record(&candidate, &record).await {
                Ok(value) => {
                    if value
                        .transaction
                        .as_ref()
                        .is_some_and(|t| !t.movement_complete)
                    {
                        let key = format!(
                            "retry:{}:{}:{}",
                            candidate.chain.key(),
                            candidate.wallet,
                            record.id
                        );
                        store.set_state(&key, &(now() + 3600).to_string())?;
                    }
                    store.save_page(candidate, &[value.clone()], &coverage, &snapshot.balances)?;
                    self.update_analysis(candidate.chain, &candidate.wallet, now())
                        .await?;
                    fetched.push(value);
                }
                Err(error) => {
                    let key = format!(
                        "retry:{}:{}:{}",
                        candidate.chain.key(),
                        candidate.wallet,
                        record.id
                    );
                    store.set_state(&key, &(now() + 3600).to_string())?;
                    record.error = Some(error);
                    store.save_page(candidate, &[record.clone()], &coverage, &snapshot.balances)?;
                    self.update_analysis(candidate.chain, &candidate.wallet, now())
                        .await?;
                    fetched.push(record);
                }
            }
        }
        store.save_page(&candidate, &fetched, &coverage, &snapshot.balances)?;
        let refreshed = self.snapshot(candidate.chain, &candidate.wallet).await?;
        let assets: BTreeSet<_> = refreshed
            .records
            .iter()
            .filter_map(|r| r.transaction.as_ref())
            .flat_map(|t| t.assets.iter().map(|d| d.asset.clone()))
            .collect();
        let balance_read = providers.balance_read(&candidate, &assets).await;
        let balance_block = balance_read.as_ref().ok().map(|(_, block)| block.clone());
        let balances = balance_read.map(|(balances, _)| balances);
        let paid_fee = refreshed
            .records
            .iter()
            .filter_map(|r| r.transaction.as_ref())
            .any(|t| {
                t.fee_asset == "SOL"
                    && t.fee_quantity
                        .is_some_and(|q| q > rust_decimal::Decimal::ZERO)
            });
        let execution = providers.execution_account(&candidate, paid_fee).await;
        let state_received = execution.is_ok() && balances.is_ok();
        match execution {
            Ok(verified) => {
                coverage.execution_account_verified = verified;
                if !verified {
                    coverage.notes.push("Execution account requires a supported ownership/fee adapter; program and protocol contracts are not trader ranks.".into());
                }
            }
            Err(error) => {
                coverage.execution_account_verified = false;
                coverage.notes.push(error);
            }
        }
        coverage.balances_reconciled = balances.is_ok();
        if balances.is_ok() {
            let at = now();
            coverage.balances_observed_at = Some(at);
            if let (Ok(received), Some(block)) = (&balances, &balance_block) {
                coverage.balance_observations = received
                    .iter()
                    .map(|(asset, quantity)| {
                        (
                            asset.clone(),
                            BalanceObservation {
                                quantity: *quantity,
                                observed_at: at,
                                block: block.clone(),
                                source: format!(
                                    "{} finalized token balance RPC",
                                    candidate.chain.label()
                                ),
                            },
                        )
                    })
                    .collect();
            }
        }
        if let Err(error) = &balances {
            coverage.notes.push(error.clone());
        }
        let balances = balances.unwrap_or(snapshot.balances);
        let txs: Vec<_> = refreshed
            .records
            .iter()
            .filter_map(|r| r.transaction.as_ref())
            .collect();
        coverage.pending_records = refreshed.records.len() - txs.len()
            + txs
                .iter()
                .filter(|t| !t.finalized || !t.movement_complete)
                .count();
        let errors: BTreeSet<_> = refreshed
            .records
            .iter()
            .filter_map(|r| r.error.as_ref())
            .collect();
        coverage.notes.extend(errors.into_iter().take(3).cloned());
        let explanations: BTreeSet<_> = txs.iter().flat_map(|t| t.notes.iter()).collect();
        coverage
            .notes
            .extend(explanations.into_iter().take(3).cloned());
        coverage.ordering_complete = txs.iter().all(|t| t.index.is_some());
        coverage.oldest_record_at = txs.iter().map(|t| t.timestamp).min();
        coverage.newest_record_at = txs.iter().map(|t| t.timestamp).max();
        let ownership_history = if matches!(candidate.chain, Chain::Solana) {
            !providers.helius_keys.is_empty() && txs.iter().all(|t| t.block >= 111_491_819)
        } else {
            matches!(candidate.chain, Chain::Robinhood)
                && providers.config.blockscout_api_key.is_some()
        };
        // Every listed record, the canonical order, ending balances, fee
        // attribution and supported execution semantics are additional gates.
        coverage.history_complete = coverage.backfill_done
            && coverage.head_complete
            && coverage.pending_records == 0
            && ownership_history;
        if state_received {
            coverage.last_state_checked_at = Some(now());
        }
        let valuation_end = coverage.last_collected_at.unwrap_or_else(now);
        store.save_page(&candidate, &[], &coverage, &balances)?;
        self.update_analysis(candidate.chain, &candidate.wallet, valuation_end)
            .await?;
        self.price(&refreshed, &assets, valuation_end).await?;
        self.update_analysis(candidate.chain, &candidate.wallet, valuation_end)
            .await?;
        Ok(())
    }

    async fn enrich_tick(&self) -> Result<(), String> {
        use std::collections::BTreeMap;
        let store = self.store.as_ref().unwrap();
        let analyses = store.analyses()?;
        if analyses.is_empty() {
            return Ok(());
        }
        let mut batches = Vec::new();
        let mut groups = Vec::new();
        for chain in [Chain::Solana, Chain::Robinhood, Chain::Bnb] {
            let cached = store.token_quotes(chain)?;
            let mut assets: BTreeMap<String, (bool, u64)> = BTreeMap::new();
            assets.insert(native(chain).into(), (true, now()));
            for analysis in analyses
                .iter()
                .filter(|a| a.candidate.chain.key() == chain.key())
            {
                for p in &analysis.positions {
                    let entry = assets.entry(p.asset.clone()).or_default();
                    entry.0 |= p.quantity > rust_decimal::Decimal::ZERO;
                    entry.1 = entry.1.max(p.last_activity_at.unwrap_or(0));
                }
                for a in &analysis.activity {
                    for asset in [a.asset.as_ref(), a.quote_asset.as_ref()]
                        .into_iter()
                        .flatten()
                    {
                        assets.entry(asset.clone()).or_insert((false, a.timestamp));
                    }
                }
                for asset in &analysis.candidate.observed_tokens {
                    assets
                        .entry(asset.clone())
                        .or_insert((false, analysis.candidate.discovered_at));
                }
            }
            let mut due: Vec<_> = assets
                .into_iter()
                .filter(|(asset, _)| {
                    cached.get(asset).is_none_or(|mark| {
                        let key = format!("market-retry:{}:{}", chain.key(), asset);
                        let retry = store
                            .state(&key)
                            .ok()
                            .flatten()
                            .and_then(|s| s.parse::<u64>().ok());
                        now()
                            >= retry.unwrap_or(
                                mark.observed_at
                                    + if mark.price_usd.is_some() { 600 } else { 3600 },
                            )
                    })
                })
                .collect();
            // Oldest mark first within priority. The whole cohort shares each
            // token read; a large wallet cannot monopolize the first 30 assets.
            due.sort_by_key(|(asset, (held, last))| {
                (
                    !*held,
                    cached.get(asset).map(|m| m.observed_at).unwrap_or(0),
                    std::cmp::Reverse(*last),
                    asset.clone(),
                )
            });
            groups.push((
                chain,
                due.into_iter()
                    .map(|(a, _)| a)
                    .collect::<std::collections::VecDeque<_>>(),
            ));
        }
        // Six batched calls per minute, spread fairly across active chains;
        // unused chain slots go to the remaining token backlog.
        while batches.len() < 6 && groups.iter().any(|(_, a)| !a.is_empty()) {
            for (chain, assets) in &mut groups {
                if batches.len() == 6 {
                    break;
                }
                let requested: Vec<_> = (0..30).filter_map(|_| assets.pop_front()).collect();
                if !requested.is_empty() {
                    batches.push((*chain, requested));
                }
            }
        }
        let results = futures::future::join_all(
            batches
                .iter()
                .map(|(chain, assets)| self.enrich_batch(*chain, assets)),
        )
        .await;
        let errors: Vec<_> = results.into_iter().filter_map(Result::err).collect();
        store.set_state("market_error", &errors.join(" "))?;
        // Historical pricing has its own fair wallet rotation and remains
        // active during a background HTTP pause. It shares Gecko's rate guard.
        let offset = store
            .state("market-wallet-cursor")?
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0)
            % analyses.len();
        store.set_state(
            "market-wallet-cursor",
            &((offset + 1) % analyses.len()).to_string(),
        )?;
        let candidate = &analyses[offset].candidate;
        let snapshot = self.snapshot(candidate.chain, &candidate.wallet).await?;
        let assets: BTreeSet<_> = snapshot
            .records
            .iter()
            .filter_map(|r| r.transaction.as_ref())
            .flat_map(|t| t.assets.iter().map(|d| d.asset.clone()))
            .collect();
        self.price(
            &snapshot,
            &assets,
            snapshot.coverage.last_collected_at.unwrap_or_else(now),
        )
        .await
    }

    async fn enrich_batch(&self, chain: Chain, requested: &[String]) -> Result<(), String> {
        let store = self.store.as_ref().unwrap();
        let cached = store.token_quotes(chain)?;
        let at = now();
        let primary = self.gecko.token_quotes(chain, requested, at).await;
        let mut errors: Vec<String> = primary
            .as_ref()
            .err()
            .map(ToString::to_string)
            .into_iter()
            .collect();
        let mut quotes = primary.unwrap_or_default();
        let missing: Vec<_> = requested
            .iter()
            .filter(|a| {
                quotes
                    .iter()
                    .find(|q| q.asset == **a)
                    .is_none_or(|q| q.price_usd.is_none())
            })
            .cloned()
            .collect();
        if !missing.is_empty() {
            let provider = self.providers.as_ref().unwrap();
            let market = crate::providers::market::MarketClient::new(
                provider.http.clone(),
                self.gecko.clone(),
                provider.config.dexscreener_api_host.clone(),
            );
            match market.token_quotes(chain, &missing, at).await {
                Ok(fallback) => {
                    for q in fallback {
                        quotes.retain(|old| old.asset != q.asset);
                        quotes.push(q);
                    }
                }
                Err(error) => errors.push(error),
            }
        }
        for quote in &mut quotes {
            if let Some(previous) = cached.get(&quote.asset) {
                quote.name = quote.name.take().or_else(|| previous.name.clone());
                quote.symbol = quote.symbol.take().or_else(|| previous.symbol.clone());
                quote.decimals = quote.decimals.or(previous.decimals);
            }
        }
        if matches!(chain, Chain::Solana) {
            let unnamed: Vec<_> = requested
                .iter()
                .filter(|a| {
                    wallet_key(chain, a).is_ok()
                        && cached
                            .get(*a)
                            .is_none_or(|q| q.name.is_none() && q.symbol.is_none())
                        && store
                            .state(&format!("metadata-retry:{}:{}", chain.key(), a))
                            .ok()
                            .flatten()
                            .and_then(|s| s.parse::<u64>().ok())
                            .is_none_or(|t| t <= at)
                        && quotes
                            .iter()
                            .find(|q| q.asset == **a)
                            .is_none_or(|q| q.name.is_none() && q.symbol.is_none())
                })
                .cloned()
                .collect();
            let mut provider = self.providers.as_ref().unwrap().clone();
            // Token identity is current evidence, not historical backfill.
            provider.lane = Some(budget::Lane::Current);
            if !provider.helius_keys.is_empty() {
                let metadata = provider.token_metadata(&unnamed).await;
                let retry = at + if metadata.is_ok() { DAY } else { 300 };
                for asset in &unnamed {
                    store.set_state(
                        &format!("metadata-retry:{}:{}", chain.key(), asset),
                        &retry.to_string(),
                    )?;
                }
                if let Ok(metadata) = metadata {
                    for q in metadata {
                        if let Some(old) = quotes.iter_mut().find(|old| old.asset == q.asset) {
                            old.name = q.name;
                            old.symbol = q.symbol;
                            old.decimals = old.decimals.or(q.decimals);
                        } else {
                            quotes.push(q);
                        }
                    }
                }
            }
        }
        for asset in requested {
            if !quotes.iter().any(|q| q.asset == *asset) {
                let old = cached.get(asset);
                quotes.push(crate::model::TokenQuote {asset:asset.clone(),name:old.and_then(|q|q.name.clone()),symbol:old.and_then(|q|q.symbol.clone()),decimals:old.and_then(|q|q.decimals),price_usd:None,observed_at:at,source:"GeckoTerminal / Dexscreener attempted".into(),detail:if errors.is_empty(){"No priced market response was indexed for this token; this does not establish zero value.".into()}else{format!("Current market evidence unavailable: {}",errors.join(" "))}});
            }
        }
        for q in &quotes {
            if q.price_usd.is_none() {
                store.set_state(
                    &format!("market-retry:{}:{}", chain.key(), q.asset),
                    &(at + if errors.is_empty() { 3600 } else { 120 }).to_string(),
                )?;
            } else {
                store.set_state(
                    &format!("market-retry:{}:{}", chain.key(), q.asset),
                    &(at + 600).to_string(),
                )?;
            }
        }
        store.save_token_quotes(chain, &quotes)?;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join(" "))
        }
    }

    async fn price(
        &self,
        snapshot: &Snapshot,
        assets: &BTreeSet<String>,
        end: u64,
    ) -> Result<(), String> {
        let store = self.store.as_ref().unwrap();
        let boundaries = [
            end.saturating_sub(60 * DAY),
            end.saturating_sub(30 * DAY),
            end,
        ];
        let mut ranked: Vec<_> = assets
            .iter()
            .map(|asset| {
                let mut times: BTreeSet<_> = snapshot
                    .records
                    .iter()
                    .filter_map(|r| r.transaction.as_ref())
                    .filter(|t| t.assets.iter().any(|d| &d.asset == asset) || &t.fee_asset == asset)
                    .map(|t| t.timestamp)
                    .collect();
                times.extend(boundaries);
                (asset.clone(), times)
            })
            .collect();
        let fee_asset = native(snapshot.candidate.chain).to_string();
        if !assets.contains(&fee_asset) {
            let mut times: BTreeSet<_> = snapshot
                .records
                .iter()
                .filter_map(|r| r.transaction.as_ref())
                .map(|t| t.timestamp)
                .collect();
            times.extend(boundaries);
            ranked.push((fee_asset, times));
        }
        ranked.sort_by_key(|(asset, times)| {
            (
                !quote(snapshot.candidate.chain, asset),
                std::cmp::Reverse(times.len()),
            )
        });
        let prices = accounting::PriceIndex::new(&snapshot.prices);
        ranked = ranked
            .into_iter()
            .filter_map(|(asset, times)| {
                let times: BTreeSet<_> = times
                    .into_iter()
                    .filter(|t| prices.get(&asset, *t).is_none())
                    .collect();
                (!times.is_empty()).then_some((asset, times))
            })
            .collect();
        let key = format!(
            "price_cursor:{}:{}",
            snapshot.candidate.chain.key(),
            snapshot.candidate.wallet
        );
        let offset = store
            .state(&key)?
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let mut nonquotes: Vec<_> = ranked
            .iter()
            .filter(|(a, _)| !quote(snapshot.candidate.chain, a))
            .cloned()
            .collect();
        if !nonquotes.is_empty() {
            let count = nonquotes.len();
            nonquotes.rotate_left(offset % count);
            store.set_state(&key, &((offset + 3) % count).to_string())?;
        }
        let mut selected: Vec<_> = ranked
            .into_iter()
            .filter(|(a, _)| quote(snapshot.candidate.chain, a))
            .take(2)
            .collect();
        selected.extend(nonquotes.into_iter().take(2));
        for (asset, times) in selected {
            let retry = format!(
                "historical-price-retry:{}:{}",
                snapshot.candidate.chain.key(),
                asset
            );
            if store
                .state(&retry)?
                .and_then(|s| s.parse::<u64>().ok())
                .is_some_and(|t| t > now())
            {
                continue;
            }
            store.set_state(&retry, &(now() + 900).to_string())?;
            let times: Vec<_> = times.into_iter().collect();
            if let Ok(prices) = self
                .gecko
                .historical_usd_prices(snapshot.candidate.chain, &asset, &times)
                .await
            {
                let prices: Vec<_> = prices
                    .into_iter()
                    .map(|(timestamp, p)| Price {
                        asset: asset.clone(),
                        timestamp,
                        usd: p.usd_price,
                        granularity: format!("{:?}", p.granularity).to_ascii_lowercase(),
                        source: "GeckoTerminal historical USD candle; estimated conversion".into(),
                    })
                    .collect();
                store.save_prices(snapshot.candidate.chain, &prices)?;
            }
            let refreshed = store
                .snapshot(snapshot.candidate.chain, &snapshot.candidate.wallet)?
                .ok_or("Wallet evidence disappeared.")?;
            let index = accounting::PriceIndex::new(&refreshed.prices);
            let missing: Vec<_> = times
                .into_iter()
                .filter(|t| index.get(&asset, *t).is_none())
                .collect();
            if let Ok(candles) = self
                .native_prices
                .historical(snapshot.candidate.chain, &asset, &missing, now())
                .await
            {
                let prices: Vec<_> = candles
                    .into_iter()
                    .map(|(timestamp, c)| Price {
                        asset: asset.clone(),
                        timestamp,
                        usd: c.usd,
                        granularity: c.granularity,
                        source: c.source,
                    })
                    .collect();
                store.save_prices(snapshot.candidate.chain, &prices)?;
            }
        }
        Ok(())
    }
}
