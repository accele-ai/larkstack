mod config;
mod debounce;
mod dispatch;
mod event;
mod sinks;
mod sources;
mod utils;

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};
use reqwest::Client;
use tracing::info;

use config::{AppState, LarkConfig, LinearConfig, ServerConfig};
use debounce::DebounceMap;

async fn health() -> &'static str {
    "ok"
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let linear = LinearConfig::from_env().expect("invalid linear config");
    let lark = LarkConfig::from_env().expect("invalid lark config");
    let server = ServerConfig::from_env().expect("invalid server config");

    let http = Client::new();
    let lark_bot = lark.bot_client(&http);
    let linear_client = linear.graphql_client(&http);

    if lark.verification_token.is_some() {
        info!("LARK_VERIFICATION_TOKEN set – event verification enabled");
    }

    info!("debounce delay: {}ms", server.debounce_delay_ms);

    let addr = format!("0.0.0.0:{}", server.port);

    let state = Arc::new(AppState {
        linear,
        lark,
        server,
        http,
        lark_bot,
        linear_client,
        update_debounce: DebounceMap::new(),
    });

    let app = Router::new()
        .route("/webhook", post(sources::linear::webhook_handler))
        .route("/lark/event", post(sinks::lark::lark_event_handler))
        .route("/health", get(health))
        .with_state(state);

    info!("listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, app).await.expect("server error");
}
