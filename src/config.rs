use reqwest::Client;

use crate::{debounce::DebounceMap, sinks::lark::LarkBotClient, sources::linear::client::LinearClient};

/// Shared application state, wrapped in `Arc` and passed to every handler.
pub struct AppState {
    pub webhook_secret: String,
    pub lark_webhook_url: String,
    pub http: Client,
    pub lark_bot: Option<LarkBotClient>,
    pub linear_client: Option<LinearClient>,
    pub lark_verification_token: Option<String>,
    pub update_debounce: DebounceMap,
    pub debounce_delay_ms: u64,
}
