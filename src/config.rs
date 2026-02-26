use reqwest::Client;

use crate::{debounce::DebounceMap, lark::LarkBotClient, linear::LinearClient};

pub struct AppState {
    pub webhook_secret: String,
    pub lark_webhook_url: String,
    pub http: Client,
    pub lark_bot: Option<LarkBotClient>,
    pub linear_client: Option<LinearClient>,
    pub lark_verification_token: Option<String>,
    pub update_debounce: DebounceMap,
}
