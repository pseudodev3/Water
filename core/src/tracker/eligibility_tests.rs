//! Policy tests exercise actual RPC reservations and persistent source retention.
use super::*;
use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use rust_decimal::Decimal;
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Default)]
struct RpcState {
    calls: Vec<String>,
    native_units: u64,
    unknown_token: bool,
    wrong_chain: bool,
}
async fn rpc(
    Path(chain): Path<String>,
    State(state): State<Arc<Mutex<RpcState>>>,
    Json(request): Json<Value>,
) -> Json<Value> {
    let mut state = state.lock().unwrap();
    let method = request["method"].as_str().unwrap();
    state.calls.push(format!("{chain}:{method}"));
    let result = match method {
        "getBalance" => json!({"context":{"slot":123},"value":state.native_units * 1_000_000_000}),
        "getTokenAccountsByOwner" => {
            let rows = if state.unknown_token
                && request["params"][1]["programId"]
                    .as_str()
                    .unwrap()
                    .starts_with("Tokenkeg")
            {
                vec![
                    json!({"account":{"data":{"parsed":{"info":{"mint":"synthetic-unpriced-token","tokenAmount":{"amount":"500","decimals":0}}}}}}),
                ]
            } else {
                vec![]
            };
            json!({"context":{"slot":124},"value":rows})
        }
        "eth_chainId" => json!(if state.wrong_chain {
            "0x1"
        } else if chain == "bnb" {
            "0x38"
        } else {
            "0x1237"
        }),
        "eth_getBlockByNumber" => {
            assert_eq!(request["params"], json!(["finalized", false]));
            json!({"number":"0x7b","hash":"0xreceived-test-block"})
        }
        "eth_getBalance" => {
            assert_eq!(request["params"][1], "0x7b");
            json!(format!(
                "0x{:x}",
                state.native_units as u128 * 1_000_000_000_000_000_000u128
            ))
        }
        "eth_call" => {
            assert_eq!(request["params"][1], "0x7b");
            assert!(request["params"][0]["data"]
                .as_str()
                .unwrap()
                .starts_with("0x70a08231"));
            json!(format!("0x{:x}", 1_000_000_000_000_000_000u128))
        }
        other => panic!("Paused wallet attempted an unexpected history/metadata call: {other}"),
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}
async fn setup(path: &str) -> (Tracker, Arc<Mutex<RpcState>>, tokio::task::JoinHandle<()>) {
    let state = Arc::new(Mutex::new(RpcState {
        native_units: 1,
        ..Default::default()
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/{chain}", post(rpc))
        .with_state(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let config = Config {
        port: 0,
        gecko_api_host: url.clone(),
        dexscreener_api_host: url.clone(),
        solana_rpc_url: format!("{url}/sol"),
        solana_fallback_rpc_url: format!("{url}/sol"),
        robinhood_rpc_url: format!("{url}/rh"),
        bnb_rpc_url: format!("{url}/bnb"),
        bnb_fallback_rpc_url: format!("{url}/bnb"),
        blockscout_api_url: url.clone(),
        blockscout_api_key: None,
    };
    let store = Arc::new(Store::open(path).unwrap());
    store
        .set_state("discovery_time", &now().to_string())
        .unwrap();
    for chain in [Chain::Solana, Chain::Robinhood, Chain::Bnb] {
        store
            .save_token_quotes(
                chain,
                &[crate::model::TokenQuote {
                    asset: native(chain).into(),
                    price_usd: Some(Decimal::from(100)),
                    observed_at: now(),
                    source: "Controlled policy-test quote".into(),
                    ..Default::default()
                }],
            )
            .unwrap();
    }
    let providers = Providers {
        http: http.clone(),
        config,
        store: store.clone(),
        helius_keys: vec![],
        helius_credit_limit: 800000,
        fomo_key: None,
        rh_trace_url: format!("{url}/rh"),
        bnb_trace_url: format!("{url}/bnb"),
        daily_limit: 2000,
        lane: Some(budget::Lane::Current),
    };
    (
        Tracker {
            store: Some(store),
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
            minimum_wallet_value: Decimal::from(1000),
        },
        state,
        server,
    )
}
fn candidate(chain: Chain, wallet: &str) -> Candidate {
    Candidate {
        chain,
        wallet: wallet.into(),
        discovered_at: now(),
        sources: vec![],
        observed_tokens: vec![],
    }
}
fn inventory(tracker: &Tracker, candidate: &Candidate, units: u64) {
    tracker
        .store
        .as_ref()
        .unwrap()
        .set_state(
            &format!("value-inventory:{}", eligibility::key(candidate)),
            &serde_json::to_string(&eligibility::Inventory {
                quantities: BTreeMap::from([(
                    native(candidate.chain).into(),
                    Decimal::from(units),
                )]),
                observed_at: now(),
                block: "received synthetic finalized block".into(),
                source: "Controlled policy-test inventory".into(),
                complete: true,
                error: None,
            })
            .unwrap(),
        )
        .unwrap();
}

#[tokio::test]
async fn low_value_stops_history_spend_preserves_sources_and_recovers_at_exact_floor() {
    let path = format!(
        "/tmp/water-capital-policy-{}-{}.sqlite",
        std::process::id(),
        now()
    );
    let (tracker, state, server) = setup(&path).await;
    let store = tracker.store.as_ref().unwrap();
    let c = candidate(Chain::Solana, "So11111111111111111111111111111111111111112");
    store.nominate(c.clone(), 4).unwrap();
    let record = Record {
        id: "retained-source".into(),
        raw: json!({"received":"keep exact source, including losing trades"}),
        transaction: None,
        error: None,
    };
    let coverage = Coverage {
        last_collected_at: Some(now() - 7200),
        ..Default::default()
    };
    store
        .save_page(&c, &[record.clone()], &coverage, &Default::default())
        .unwrap();
    tracker
        .update_analysis(c.chain, &c.wallet, now())
        .await
        .unwrap();
    let saved =
        serde_json::to_value(store.record(c.chain, &c.wallet, &record.id).unwrap()).unwrap();
    tracker.tick().await.unwrap();
    let v = tracker.wallet_value(&c).unwrap();
    assert_eq!(v.status, "below_minimum");
    assert_eq!(v.known_value_usd, Some(Decimal::from(100)));
    assert!(v.total_complete);
    assert!(v.next_check_at >= now() + 6 * 3600 - 2);
    assert!(!tracker.collection_admitted(&c).unwrap());
    assert_eq!(state.lock().unwrap().calls.len(), 3);
    tracker.tick().await.unwrap();
    assert!(!tracker.screen_collection(&c).await.unwrap());
    assert_eq!(state.lock().unwrap().calls.len(), 3);
    assert_eq!(store.lane_used(budget::Lane::Current, now()).unwrap(), 3);
    assert_eq!(store.lane_used(budget::Lane::History, now()).unwrap(), 0);
    assert_eq!(
        serde_json::to_value(store.record(c.chain, &c.wallet, &record.id).unwrap()).unwrap(),
        saved
    );
    assert_eq!(
        store
            .snapshot(c.chain, &c.wallet)
            .unwrap()
            .unwrap()
            .coverage
            .last_collected_at,
        coverage.last_collected_at
    );
    // The gate and schedule survive reopening the persistent store.
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened
            .state(&format!("value-inventory:{}", eligibility::key(&c)))
            .unwrap(),
        store
            .state(&format!("value-inventory:{}", eligibility::key(&c)))
            .unwrap()
    );
    assert_eq!(
        reopened
            .record(c.chain, &c.wallet, &record.id)
            .unwrap()
            .unwrap()
            .raw,
        record.raw
    );
    drop(reopened);
    state.lock().unwrap().unknown_token = true;
    store
        .set_state(&format!("value-next:{}", eligibility::key(&c)), "0")
        .unwrap();
    assert!(!tracker.screen_collection(&c).await.unwrap());
    let v = tracker.wallet_value(&c).unwrap();
    assert_eq!(v.status, "awaiting_value");
    assert_eq!(v.unpriced_assets, 1);
    assert_eq!(v.known_value_usd, Some(Decimal::from(100)));
    state.lock().unwrap().native_units = 10;
    let before = tracker.active_wallet_keys().unwrap();
    store
        .set_state(&format!("value-next:{}", eligibility::key(&c)), "0")
        .unwrap();
    assert!(tracker.screen_collection(&c).await.unwrap());
    assert!(tracker.collection_admitted(&c).unwrap());
    assert_eq!(state.lock().unwrap().calls.len(), 7); // 3 + 3 + native-only 1
    assert_eq!(
        tracker.wallet_value(&c).unwrap().known_value_usd,
        Some(Decimal::from(1000))
    );
    assert!(!tracker.wallet_value(&c).unwrap().total_complete);
    tracker.wake_newly_admitted(&before).unwrap();
    assert!(store.next_wallet(now(), 360).unwrap().is_some());
    server.abort();
    drop(tracker);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{path}{suffix}"));
    }
}

#[tokio::test]
async fn bnb_and_rh_native_value_use_verified_chain_and_received_finalized_block() {
    let (tracker, state, server) = setup(":memory:").await;
    for chain in [Chain::Bnb, Chain::Robinhood] {
        let c = candidate(chain, "0x1111111111111111111111111111111111111111");
        tracker
            .store
            .as_ref()
            .unwrap()
            .nominate(c.clone(), 4)
            .unwrap();
        assert!(!tracker.screen_collection(&c).await.unwrap());
        assert_eq!(tracker.wallet_value(&c).unwrap().status, "awaiting_value"); // partial EVM inventory cannot prove low total
        assert_eq!(
            tracker.wallet_value(&c).unwrap().known_value_usd,
            Some(Decimal::from(100))
        );
        state.lock().unwrap().native_units = 10;
        tracker
            .store
            .as_ref()
            .unwrap()
            .set_state(&format!("value-next:{}", eligibility::key(&c)), "0")
            .unwrap();
        assert!(tracker.screen_collection(&c).await.unwrap());
        assert_eq!(
            tracker.wallet_value(&c).unwrap().balance_block.as_deref(),
            Some("0x7b")
        );
        state.lock().unwrap().wrong_chain = true;
        tracker
            .store
            .as_ref()
            .unwrap()
            .set_state(&format!("value-next:{}", eligibility::key(&c)), "0")
            .unwrap();
        assert!(!tracker.screen_collection(&c).await.unwrap());
        assert_eq!(tracker.wallet_value(&c).unwrap().known_value_usd, None);
        assert!(tracker
            .wallet_value(&c)
            .unwrap()
            .detail
            .contains("outside chain"));
        let mut s = state.lock().unwrap();
        s.native_units = 1;
        s.wrong_chain = false;
    }
    assert_eq!(state.lock().unwrap().calls.len(), 14); // per chain: 3 + 3 + rejected identity 1
    server.abort();
}

#[tokio::test]
async fn screening_pool_keeps_history_fair_and_only_top_capital_wallets_collect() {
    let (tracker, _, server) = setup(":memory:").await;
    let store = tracker.store.as_ref().unwrap();
    let low = candidate(Chain::Solana, "a-low");
    let first = candidate(Chain::Solana, "b-eligible");
    let top = candidate(Chain::Solana, "c-largest");
    for (c, units) in [(&low, 1), (&first, 10), (&top, 20)] {
        store.nominate(c.clone(), 4).unwrap();
        inventory(&tracker, c, units);
    }
    let keys = tracker.active_wallet_keys().unwrap();
    assert_eq!(keys, BTreeSet::from([eligibility::key(&top)]));
    assert_eq!(
        store
            .next_admitted_history_wallet(now(), &keys)
            .unwrap()
            .unwrap()
            .wallet,
        top.wallet
    );
    assert!(store
        .next_admitted_history_wallet(now(), &keys)
        .unwrap()
        .is_none());
    assert_eq!(store.wallet_count().unwrap(), 3); // no evidence or candidate deletion
    assert_eq!(store.lane_used(budget::Lane::History, now()).unwrap(), 0);
    server.abort();
}

#[tokio::test]
async fn bnb_and_rh_known_token_value_combines_with_native_without_claiming_full_inventory() {
    let (tracker, state, server) = setup(":memory:").await;
    let store = tracker.store.as_ref().unwrap();
    let token = "0x2222222222222222222222222222222222222222";
    for chain in [Chain::Bnb, Chain::Robinhood] {
        let mut c = candidate(chain, "0x1111111111111111111111111111111111111111");
        c.observed_tokens.push(token.into());
        store.nominate(c.clone(), 4).unwrap();
        store
            .save_token_quotes(
                chain,
                &[crate::model::TokenQuote {
                    asset: token.into(),
                    decimals: Some(18),
                    price_usd: Some(Decimal::from(900)),
                    observed_at: now(),
                    source: "Controlled token mark".into(),
                    ..Default::default()
                }],
            )
            .unwrap();
        assert!(tracker.screen_collection(&c).await.unwrap());
        let value = tracker.wallet_value(&c).unwrap();
        assert_eq!(value.known_value_usd, Some(Decimal::from(1000)));
        assert_eq!(value.positive_assets, 2);
        assert!(!value.inventory_complete);
        assert!(!value.total_complete);
        assert_eq!(value.balance_block.as_deref(), Some("0x7b"));
    }
    assert_eq!(state.lock().unwrap().calls.len(), 8); // each: verified identity/block/native + one known ERC20 balance
    server.abort();
}
