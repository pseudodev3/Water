mod chains;
mod config;
mod engine;
mod model;
mod providers;

use axum::{
    extract::State,
    http::{header::CONTENT_TYPE, HeaderValue, Method, StatusCode},
    routing::{get, post},
    Json, Router,
};
use config::Config;
use model::{ScanRequest, SourceStatus};
use providers::gmgn::{GmgnClient, GmgnError};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;

#[derive(Clone)]
struct AppState {
    config: Config,
    http: reqwest::Client,
    gmgn: GmgnClient,
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
    let gmgn = GmgnClient::new(
        http.clone(),
        config.gmgn_api_key.clone(),
        config.gmgn_api_host.clone(),
    );

    let state = Arc::new(AppState {
        config: config.clone(),
        http,
        gmgn,
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

async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "gmgn_configured": state.gmgn.configured(),
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

    if !state.gmgn.configured() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "GMGN_API_KEY is not configured on the Water core." })),
        ));
    }

    let chain = request.chain;
    let address = request.address.trim().to_string();

    let direct_verification =
        chains::verify_asset(&state.http, &state.config, chain, &address);
    let info_request = state.gmgn.token_info(chain, &address);
    let security_request = state.gmgn.token_security(chain, &address);
    let pool_request = state.gmgn.token_pool(chain, &address);

    let (chain_evidence, info, security, pool) = tokio::join!(
        direct_verification,
        info_request,
        security_request,
        pool_request
    );

    // These endpoints have heavier GMGN weights. Keep them sequential and
    // let the provider do one bounded retry on a 429.
    let holders = state.gmgn.top_holders(chain, &address).await;
    let traders = state.gmgn.top_traders(chain, &address).await;

    let sources = vec![
        source_status("GMGN token info", &info),
        source_status("GMGN token security", &security),
        source_status("GMGN pool info", &pool),
        source_status("GMGN top holders", &holders),
        source_status("GMGN top traders", &traders),
    ];

    if sources.iter().all(|source| !source.ok) {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": "GMGN did not return usable data for this scan.",
                "sources": sources
            })),
        ));
    }

    Ok(Json(engine::build_scan(
        &request,
        info.as_ref().ok(),
        pool.as_ref().ok(),
        holders.as_ref().ok(),
        traders.as_ref().ok(),
        chain_evidence,
        sources,
    )))
}

fn source_status(name: &str, result: &Result<Value, GmgnError>) -> SourceStatus {
    match result {
        Ok(_) => SourceStatus {
            source: name.to_string(),
            ok: true,
            detail: "Received.".to_string(),
        },
        Err(error) => SourceStatus {
            source: name.to_string(),
            ok: false,
            detail: error.to_string(),
        },
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
