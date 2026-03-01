use figment::{Figment, providers::Env};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    debounce::DebounceMap, sinks::lark::LarkBotClient, sources::linear::client::LinearClient,
};

#[derive(Debug, Deserialize, Serialize)]
pub struct LinearConfig {
    pub webhook_secret: String,
    pub api_key: Option<String>,
}

impl LinearConfig {
    pub fn from_env() -> Result<Self, Box<figment::Error>> {
        Figment::new()
            .merge(Env::prefixed("LINEAR_"))
            .extract()
            .map_err(Box::new)
    }

    pub fn graphql_client(&self, http: &Client) -> Option<LinearClient> {
        self.api_key.as_ref().map(|key| {
            info!("LINEAR_API_KEY set – link preview enabled");
            LinearClient::new(key.clone(), http.clone())
        })
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct LarkConfig {
    #[serde(default)]
    pub webhook_url: String,
    pub app_id: Option<String>,
    pub app_secret: Option<String>,
    pub verification_token: Option<String>,
}

impl LarkConfig {
    pub fn from_env() -> Result<Self, Box<figment::Error>> {
        Figment::new()
            .merge(figment::providers::Serialized::defaults(Self::default()))
            .merge(Env::prefixed("LARK_"))
            .extract()
            .map_err(Box::new)
    }

    pub fn bot_client(&self, http: &Client) -> Option<LarkBotClient> {
        match (&self.app_id, &self.app_secret) {
            (Some(id), Some(secret)) => {
                info!("lark bot configured – DM notifications enabled");
                Some(LarkBotClient::new(id.clone(), secret.clone(), http.clone()))
            }
            _ => {
                info!("LARK_APP_ID/LARK_APP_SECRET not set – DM notifications disabled");
                None
            }
        }
    }
}

fn default_port() -> u16 {
    3000
}

fn default_debounce() -> u64 {
    5000
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_debounce")]
    pub debounce_delay_ms: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            debounce_delay_ms: default_debounce(),
        }
    }
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, Box<figment::Error>> {
        Figment::new()
            .merge(figment::providers::Serialized::defaults(Self::default()))
            .merge(Env::raw().only(&["PORT", "DEBOUNCE_DELAY_MS"]))
            .extract()
            .map_err(Box::new)
    }
}

/// Shared application state, wrapped in `Arc` and passed to every handler.
pub struct AppState {
    pub linear: LinearConfig,
    pub lark: LarkConfig,
    pub server: ServerConfig,
    pub http: Client,
    pub lark_bot: Option<LarkBotClient>,
    pub linear_client: Option<LinearClient>,
    pub update_debounce: DebounceMap,
}

impl AppState {
    pub fn from_env() -> Self {
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

        Self {
            linear,
            lark,
            server,
            http,
            lark_bot,
            linear_client,
            update_debounce: DebounceMap::new(),
        }
    }
}
