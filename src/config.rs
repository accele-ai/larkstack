#[cfg(not(feature = "cf-worker"))]
use figment::{Figment, providers::Env};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{sinks::lark::LarkBotClient, sources::linear::client::LinearClient};

#[cfg(not(feature = "cf-worker"))]
use crate::debounce::DebounceMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct LinearConfig {
    pub webhook_secret: String,
    pub api_key: Option<String>,
}

#[cfg(not(feature = "cf-worker"))]
impl LinearConfig {
    pub fn from_env() -> Result<Self, Box<figment::Error>> {
        Figment::new()
            .merge(Env::prefixed("LINEAR_"))
            .extract()
            .map_err(Box::new)
    }
}

#[cfg(feature = "cf-worker")]
impl LinearConfig {
    pub fn from_worker_env(env: &worker::Env) -> Result<Self, String> {
        Ok(Self {
            webhook_secret: env
                .secret("LINEAR_WEBHOOK_SECRET")
                .map_err(|e| format!("LINEAR_WEBHOOK_SECRET: {e}"))?
                .to_string(),
            api_key: env.secret("LINEAR_API_KEY").ok().map(|s| s.to_string()),
        })
    }
}

impl LinearConfig {
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

#[cfg(not(feature = "cf-worker"))]
impl LarkConfig {
    pub fn from_env() -> Result<Self, Box<figment::Error>> {
        Figment::new()
            .merge(figment::providers::Serialized::defaults(Self::default()))
            .merge(Env::prefixed("LARK_"))
            .extract()
            .map_err(Box::new)
    }
}

#[cfg(feature = "cf-worker")]
impl LarkConfig {
    pub fn from_worker_env(env: &worker::Env) -> Result<Self, String> {
        Ok(Self {
            webhook_url: env
                .var("LARK_WEBHOOK_URL")
                .map(|v| v.to_string())
                .unwrap_or_default(),
            app_id: env.var("LARK_APP_ID").ok().map(|v| v.to_string()),
            app_secret: env.secret("LARK_APP_SECRET").ok().map(|s| s.to_string()),
            verification_token: env
                .secret("LARK_VERIFICATION_TOKEN")
                .ok()
                .map(|s| s.to_string()),
        })
    }
}

impl LarkConfig {
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

#[cfg(not(feature = "cf-worker"))]
impl ServerConfig {
    pub fn from_env() -> Result<Self, Box<figment::Error>> {
        Figment::new()
            .merge(figment::providers::Serialized::defaults(Self::default()))
            .merge(Env::raw().only(&["PORT", "DEBOUNCE_DELAY_MS"]))
            .extract()
            .map_err(Box::new)
    }
}

#[cfg(feature = "cf-worker")]
impl ServerConfig {
    pub fn from_worker_env(env: &worker::Env) -> Result<Self, String> {
        Ok(Self {
            port: env
                .var("PORT")
                .ok()
                .and_then(|v| v.to_string().parse().ok())
                .unwrap_or_else(default_port),
            debounce_delay_ms: env
                .var("DEBOUNCE_DELAY_MS")
                .ok()
                .and_then(|v| v.to_string().parse().ok())
                .unwrap_or_else(default_debounce),
        })
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
    #[cfg(not(feature = "cf-worker"))]
    pub update_debounce: DebounceMap,
    #[cfg(feature = "cf-worker")]
    pub env: worker::Env,
}

#[cfg(not(feature = "cf-worker"))]
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

#[cfg(feature = "cf-worker")]
impl AppState {
    pub fn from_worker_env(env: worker::Env) -> Self {
        let linear = LinearConfig::from_worker_env(&env).expect("invalid linear config");
        let lark = LarkConfig::from_worker_env(&env).expect("invalid lark config");
        let server = ServerConfig::from_worker_env(&env).expect("invalid server config");

        let http = Client::new();
        let lark_bot = lark.bot_client(&http);
        let linear_client = linear.graphql_client(&http);

        Self {
            linear,
            lark,
            server,
            http,
            lark_bot,
            linear_client,
            env,
        }
    }
}
