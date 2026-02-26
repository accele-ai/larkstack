mod config;
mod debounce;
mod handlers;
mod lark;
mod linear;
mod models;
mod utils;

use std::{env, sync::Arc};

use axum::{
    routing::{get, post},
    Router,
};
use reqwest::Client;
use tracing::{info, warn};

use config::AppState;
use debounce::DebounceMap;
use handlers::{health, lark_event_handler, webhook_handler};
use lark::LarkBotClient;
use linear::LinearClient;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let webhook_secret =
        env::var("LINEAR_WEBHOOK_SECRET").expect("LINEAR_WEBHOOK_SECRET must be set");
    let lark_webhook_url = env::var("LARK_WEBHOOK_URL").unwrap_or_else(|_| {
        warn!("LARK_WEBHOOK_URL not set – lark notifications will fail");
        String::new()
    });
    let port = env::var("PORT").unwrap_or_else(|_| "3000".into());

    let lark_bot = match (env::var("LARK_APP_ID"), env::var("LARK_APP_SECRET")) {
        (Ok(app_id), Ok(app_secret)) => {
            info!("lark bot configured – DM notifications enabled");
            Some(LarkBotClient::new(app_id, app_secret, Client::new()))
        }
        _ => {
            info!("LARK_APP_ID/LARK_APP_SECRET not set – DM notifications disabled");
            None
        }
    };

    let linear_client = env::var("LINEAR_API_KEY").ok().map(|api_key| {
        info!("LINEAR_API_KEY set – link preview enabled");
        LinearClient::new(api_key, Client::new())
    });

    let lark_verification_token = env::var("LARK_VERIFICATION_TOKEN").ok();
    if lark_verification_token.is_some() {
        info!("LARK_VERIFICATION_TOKEN set – event verification enabled");
    }

    let state = Arc::new(AppState {
        webhook_secret,
        lark_webhook_url,
        http: Client::new(),
        lark_bot,
        linear_client,
        lark_verification_token,
        update_debounce: DebounceMap::new(),
    });

    let app = Router::new()
        .route("/webhook", post(webhook_handler))
        .route("/lark/event", post(lark_event_handler))
        .route("/health", get(health))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    info!("listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, app).await.expect("server error");
}
