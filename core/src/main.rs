mod chains;
mod cohort;
mod config;
mod early;
mod early_map;
mod flow;
mod history;
mod engine;
mod ledger;
mod model;
mod origin;
mod robinhood_launchpads;
mod providers;
mod position;
mod reconstruct;

use axum::{
    extract::State,
    http::{header::CONTENT_TYPE, Method, StatusCode},
    routing::{get, post},
    Json, Router,
};
use config::Config;
use model::{Chain, ScanRequest, SourceStatus};
use providers::gecko::GeckoClient;
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::{cors::{Any, CorsLayer}, trace::TraceLayer};
use tracing::info;
use tokio::time::{timeout, Duration};

#[derive(Clone)]
struct AppState {
    config: Config,
    http: reqwest::Client,
    gecko: GeckoClient,
    market: providers::market::MarketClient,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "water_core=info,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .expect("HTTP client should initialize");
    let gecko = GeckoClient::new(http.clone(), config.gecko_api_host.clone());

    let market = providers::market::MarketClient::new(http.clone(), gecko.clone(), config.dexscreener_api_host.clone());
    let state = Arc::new(AppState { config: config.clone(), http, gecko, market });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE]);

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/scan", post(scan))
        .route("/v1/wallet-position", post(wallet_position))
        .route("/v1/holder-cohort", post(holder_cohort))
        .route("/v1/early-holders", post(early_holders))
        .route("/v1/origin", post(origin))
        .route("/v1/token-info", post(token_info))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.port))
        .await
        .expect("Water should bind to the configured port");

    info!("Water core listening on {}", config.port);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Water server should run");
}

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "market_provider": "GeckoTerminal public API",
        "market_fallback": "Dexscreener public API",
        "requires_market_api_key": false,
        "chains": ["solana", "robinhood"]
    }))
}

async fn scan(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ScanRequest>,
) -> Result<Json<model::ScanResponse>, (StatusCode, Json<Value>)> {
    if let Err(message) = request.validate() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": message }))));
    }

    let chain = request.chain;
    let address = request.address.trim().to_string();

    let market_request = async {
        timeout(Duration::from_secs(12), state.market.snapshot(chain, &address)).await
            .unwrap_or_else(|_| providers::market::MarketRead {
                snapshot: None,
                sources: vec![SourceStatus {
                    source: "Public market data".to_string(), ok: false,
                    detail: "Market providers exceeded the 12s scan budget; available chain evidence is still returned.".to_string(),
                }],
            })
    };
    let chain_request = chains::observe_asset(&state.http, &state.config, chain, &address);
    let (market_read, (chain_evidence, holder_evidence)) =
        tokio::join!(market_request, chain_request);

    let mut sources = market_read.sources;
    sources.push(SourceStatus {
        source: "Direct chain verification".to_string(),
        ok: chain_evidence.verified,
        detail: chain_evidence.detail.clone(),
    });

    match chain {
        Chain::Solana => {
            sources.push(SourceStatus {
                source: "Solana getTokenSupply".to_string(),
                ok: holder_evidence.total_supply.is_some(),
                detail: holder_evidence.detail.clone(),
            });
            sources.push(SourceStatus {
                source: "Solana wallet-holder reconstruction".to_string(),
                ok: holder_evidence.top_ten_percentage.is_some(),
                detail: holder_evidence.detail.clone(),
            });
        }
        Chain::Robinhood => {
            sources.push(SourceStatus {
                source: "Robinhood ERC-20 totalSupply".to_string(),
                ok: holder_evidence.total_supply.is_some(),
                detail: holder_evidence.detail.clone(),
            });
            sources.push(SourceStatus {
                source: "Robinhood indexed wallet holders".to_string(),
                ok: holder_evidence.top_ten_percentage.is_some(),
                detail: holder_evidence.detail.clone(),
            });
        }
    }

    Ok(Json(engine::build_scan(
        &request,
        market_read.snapshot.as_ref(),
        holder_evidence,
        chain_evidence,
        sources,
    )))
}


async fn wallet_position(
    State(state): State<Arc<AppState>>,
    Json(request): Json<position::WalletPositionRequest>,
) -> Result<Json<position::WalletPositionResponse>, (StatusCode, Json<Value>)> {
    position::analyze_wallet_position(
        state.http.clone(),
        &state.config,
        &state.gecko,
        request,
    )
    .await
    .map(Json)
    .map_err(|message| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": message })),
        )
    })
}


async fn holder_cohort(
    State(state): State<Arc<AppState>>,
    Json(request): Json<cohort::HolderCohortRequest>,
) -> Result<Json<cohort::HolderCohortResponse>, (StatusCode, Json<Value>)> {
    cohort::analyze_holder_cohort(
        state.http.clone(),
        &state.config,
        &state.gecko,
        request,
    )
    .await
    .map(Json)
    .map_err(|message| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": message })),
        )
    })
}

async fn early_holders(
    State(state): State<Arc<AppState>>,
    Json(request): Json<early_map::EarlyHolderMapRequest>,
) -> Result<Json<early_map::EarlyHolderMapResponse>, (StatusCode, Json<Value>)> {
    match timeout(
        Duration::from_secs(24),
        early_map::analyze_early_holder_map(
            state.http.clone(),
            &state.config,
            &state.gecko,
            request,
        ),
    )
    .await
    {
        Ok(Ok(response)) => Ok(Json(response)),
        Ok(Err(message)) => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": message })),
        )),
        Err(_) => Err((
            StatusCode::REQUEST_TIMEOUT,
            Json(json!({
                "error": "Early-holder history exceeded Water's verification budget."
            })),
        )),
    }
}

async fn origin(
    State(state): State<Arc<AppState>>,
    Json(request): Json<origin::OriginRequest>,
) -> Result<Json<origin::OriginResponse>, (StatusCode, Json<Value>)> {
    match timeout(
        Duration::from_secs(10),
        origin::inspect_origin(state.http.clone(), &state.config, request),
    )
    .await
    {
        Ok(Ok(response)) => Ok(Json(response)),
        Ok(Err(message)) => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": message })),
        )),
        Err(_) => Err((
            StatusCode::REQUEST_TIMEOUT,
            Json(json!({ "error": "Origin evidence exceeded Water's verification budget." })),
        )),
    }
}

async fn token_info(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ScanRequest>,
) -> Result<Json<providers::gecko::TokenInfoSnapshot>, (StatusCode, Json<Value>)> {
    if let Err(message) = request.validate() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": message }))));
    }

    state
        .gecko
        .token_info(request.chain, request.address.trim())
        .await
        .map(Json)
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": error.to_string() })),
            )
        })
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod scan_regression_tests {
    use super::*;
    use std::time::Instant;

    async fn mock_state(stall_market: bool, stall_all_rpc: bool) -> (Arc<AppState>, tokio::task::JoinHandle<()>) {
        let rpc = move |Json(body): Json<Value>| async move {
            let method = body["method"].as_str().unwrap_or_default();
            if stall_all_rpc || method == "getProgramAccounts" {
                std::future::pending::<()>().await;
            }
            let value = match method {
                "getAccountInfo" => json!({"value":{"owner":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","data":["", "base64"]}}),
                "getTokenSupply" => json!({"value":{"uiAmountString":"1000000000","amount":"1000000000000000","decimals":6}}),
                _ => Value::Null,
            };
            Json(json!({"jsonrpc":"2.0","id":1,"result":value}))
        };
        let market = move || async move {
            if stall_market { std::future::pending::<()>().await; }
            Json(json!({"data":{"attributes":{"name":"Regression fixture","price_usd":"0.002","market_cap_usd":"0","total_reserve_in_usd":"50000"}}}))
        };
        let app = Router::new().route("/", post(rpc)).route("/networks/solana/tokens/{token}", get(market));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let config = Config { port:0, gecko_api_host:url.clone(), dexscreener_api_host:url.clone(), solana_rpc_url:url.clone(), solana_fallback_rpc_url:url.clone(), robinhood_rpc_url:url.clone(), blockscout_api_url:url.clone(), blockscout_api_key:None };
        let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().unwrap();
        let gecko = GeckoClient::new(http.clone(), url.clone());
        let market = providers::market::MarketClient::new(http.clone(), gecko.clone(), url);
        (Arc::new(AppState { gecko, market, http, config }), task)
    }

    #[tokio::test]
    async fn slow_holders_preserve_market_supply_and_chain_evidence() {
        let (state, task) = mock_state(false, false).await;
        let started = Instant::now();
        let Json(result) = scan(State(state), Json(ScanRequest {chain:Chain::Solana,address:"So11111111111111111111111111111111111111112".to_string()})).await.unwrap();
        task.abort();
        assert!(started.elapsed() < Duration::from_secs(11));
        assert_eq!(result.token.market_cap_usd, Some(2_000_000.0));
        assert_eq!(result.token.market_cap_basis, "supply_implied");
        assert_eq!(result.holder_evidence.total_supply, Some(1_000_000_000.0));
        assert!(result.chain_evidence.verified);
        assert!(result.holder_evidence.top_ten_percentage.is_none());
        assert!(result.holder_evidence.detail.contains("9s scan budget"));
    }

    #[tokio::test]
    async fn stalled_providers_finish_before_frontend_deadline_without_fake_zeroes() {
        let (state, task) = mock_state(true, true).await;
        let started = Instant::now();
        let Json(result) = scan(State(state), Json(ScanRequest {chain:Chain::Solana,address:"So11111111111111111111111111111111111111112".to_string()})).await.unwrap();
        task.abort();
        assert!(started.elapsed() < Duration::from_secs(14));
        assert!(result.token.price_usd.is_none());
        assert!(result.token.market_cap_usd.is_none());
        assert_eq!(result.token.market_cap_basis, "unavailable");
        assert!(!result.chain_evidence.verified);
        assert!(result.sources.iter().all(|s| !s.ok));
        assert!(result.sources.iter().any(|s| s.source.contains("Dexscreener")));
    }
}
