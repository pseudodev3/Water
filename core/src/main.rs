mod chains;
mod config;
mod early;
mod flow;
mod history;
mod engine;
mod ledger;
mod model;
mod providers;
mod reconstruct;

use axum::{
    extract::State,
    http::{header::CONTENT_TYPE, HeaderValue, Method, StatusCode},
    routing::{get, post},
    Json, Router,
};
use config::Config;
use model::{ScanRequest, SourceStatus};
use providers::gecko::GeckoClient;
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;

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

    let origin = config
        .allowed_origin
        .parse::<HeaderValue>()
        .expect("WATER_ALLOWED_ORIGIN must be a valid origin");

    let cors = CorsLayer::new()
        .allow_origin(origin)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE]);

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/scan", post(scan))
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

    let mut sources = vec![
        SourceStatus {
            source: "Direct chain verification".to_string(),
            ok: chain_evidence.verified,
            detail: chain_evidence.detail.clone(),
        },
        SourceStatus {
            source: holder_evidence.source.clone(),
            ok: holder_evidence.top_ten_percentage.is_some(),
            detail: holder_evidence.detail.clone(),
        },
    ];

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

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
