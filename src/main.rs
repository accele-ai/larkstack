use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};
use tracing::info;

use larkstack::config::AppState;

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

    let state = Arc::new(AppState::from_env());
    let addr = format!("0.0.0.0:{}", state.server.port);

    let app = Router::new()
        .route(
            "/webhook",
            post(larkstack::sources::linear::webhook_handler),
        )
        .route(
            "/lark/event",
            post(larkstack::sinks::lark::lark_event_handler),
        )
        .route("/health", get(health))
        .with_state(state);

    info!("listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, app).await.expect("server error");
}
