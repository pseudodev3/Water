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

    let state = Arc::new(AppState {
        config: config.clone(),
        http,
        gecko,
    });

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

    let market_request = state.gecko.market_snapshot(chain, &address);
    let chain_request = chains::observe_asset(&state.http, &state.config, chain, &address);
    let (market_result, (chain_evidence, holder_evidence)) =
        tokio::join!(market_request, chain_request);

    let mut sources = vec![SourceStatus {
        source: "Direct chain verification".to_string(),
        ok: chain_evidence.verified,
        detail: chain_evidence.detail.clone(),
    }];

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

    match &market_result {
        Ok(_) => sources.insert(
            0,
            SourceStatus {
                source: "GeckoTerminal public market data".to_string(),
                ok: true,
                detail: "Token and top-pool market data received.".to_string(),
            },
        ),
        Err(error) => sources.insert(
            0,
            SourceStatus {
                source: "GeckoTerminal public market data".to_string(),
                ok: false,
                detail: error.to_string(),
            },
        ),
    }

    Ok(Json(engine::build_scan(
        &request,
        market_result.as_ref().ok(),
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
