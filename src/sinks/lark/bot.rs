//! Lark Bot API client for sending direct messages via tenant access token.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::info;

#[cfg(not(feature = "cf-worker"))]
use std::time::Instant;
#[cfg(feature = "cf-worker")]
use web_time::Instant;

use super::models::LarkCard;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct TokenRequest<'a> {
    app_id: &'a str,
    app_secret: &'a str,
}

#[derive(Deserialize)]
struct TokenResponse {
    code: i64,
    tenant_access_token: Option<String>,
    #[serde(default = "default_expire")]
    expire: u64,
}

fn default_expire() -> u64 {
    7200
}

#[derive(Serialize)]
struct SendMessagePayload {
    receive_id: String,
    msg_type: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct LarkApiResponse {
    code: i64,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Authenticated Lark bot that can send interactive-card DMs.
pub struct LarkBotClient {
    app_id: String,
    app_secret: String,
    token: Mutex<CachedToken>,
    http: Client,
}

struct CachedToken {
    value: String,
    expires_at: Instant,
}

impl LarkBotClient {
    pub fn new(app_id: String, app_secret: String, http: Client) -> Self {
        Self {
            app_id,
            app_secret,
            token: Mutex::new(CachedToken {
                value: String::new(),
                expires_at: Instant::now(),
            }),
            http,
        }
    }

    /// Returns a valid tenant access token, refreshing it when necessary.
    async fn get_token(&self) -> Result<String, String> {
        let mut cached = self.token.lock().await;

        if !cached.value.is_empty()
            && cached.expires_at > Instant::now() + std::time::Duration::from_secs(300)
        {
            return Ok(cached.value.clone());
        }

        let req = TokenRequest {
            app_id: &self.app_id,
            app_secret: &self.app_secret,
        };

        let resp = self
            .http
            .post("https://open.larksuite.com/open-apis/auth/v3/tenant_access_token/internal")
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("token request failed: {e}"))?;

        let token_resp: TokenResponse = resp
            .json()
            .await
            .map_err(|e| format!("token response parse failed: {e}"))?;

        if token_resp.code != 0 {
            return Err(format!("token API error code {}", token_resp.code));
        }

        let token = token_resp
            .tenant_access_token
            .ok_or_else(|| "missing tenant_access_token in response".to_string())?;

        cached.value = token.clone();
        cached.expires_at = Instant::now() + std::time::Duration::from_secs(token_resp.expire);

        info!(
            "refreshed lark bot tenant access token (expires in {}s)",
            token_resp.expire
        );
        Ok(token)
    }

    /// Sends an interactive card message and checks the response code.
    async fn send_card(&self, url: &str, receive_id: &str, card: &LarkCard) -> Result<(), String> {
        let token = self.get_token().await?;

        let payload = SendMessagePayload {
            receive_id: receive_id.to_string(),
            msg_type: "interactive",
            content: serde_json::to_string(card).unwrap_or_default(),
        };

        let resp = self
            .http
            .post(url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("send_card request failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(format!("send_card returned {status}: {body}"));
        }

        let api_resp: LarkApiResponse =
            serde_json::from_str(&body).map_err(|e| format!("response parse failed: {e}"))?;

        if api_resp.code != 0 {
            return Err(format!("Lark API returned code {}: {body}", api_resp.code));
        }

        Ok(())
    }

    /// Sends an interactive card to a user identified by `email`.
    pub async fn send_dm(&self, email: &str, card: &LarkCard) -> Result<(), String> {
        self.send_card(
            "https://open.larksuite.com/open-apis/im/v1/messages?receive_id_type=email",
            email,
            card,
        )
        .await
        .inspect(|()| info!("DM sent to {email}"))
    }

    /// Sends an interactive card to a group chat identified by `chat_id`.
    pub async fn send_to_chat(&self, chat_id: &str, card: &LarkCard) -> Result<(), String> {
        self.send_card(
            "https://open.larksuite.com/open-apis/im/v1/messages?receive_id_type=chat_id",
            chat_id,
            card,
        )
        .await
        .inspect(|()| info!("card sent to chat {chat_id}"))
    }
}
