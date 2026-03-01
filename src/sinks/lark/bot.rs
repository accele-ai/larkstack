//! Lark Bot API client for sending direct messages via tenant access token.

use reqwest::Client;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::info;

use super::models::LarkCard;

/// Authenticated Lark bot that can send interactive-card DMs.
pub struct LarkBotClient {
    app_id: String,
    app_secret: String,
    token: Mutex<CachedToken>,
    http: Client,
}

struct CachedToken {
    value: String,
    expires_at: std::time::Instant,
}

impl LarkBotClient {
    pub fn new(app_id: String, app_secret: String, http: Client) -> Self {
        Self {
            app_id,
            app_secret,
            token: Mutex::new(CachedToken {
                value: String::new(),
                expires_at: std::time::Instant::now(),
            }),
            http,
        }
    }

    /// Returns a valid tenant access token, refreshing it when necessary.
    async fn get_token(&self) -> Result<String, String> {
        let mut cached = self.token.lock().await;

        if !cached.value.is_empty()
            && cached.expires_at > std::time::Instant::now() + std::time::Duration::from_secs(300)
        {
            return Ok(cached.value.clone());
        }

        let resp = self
            .http
            .post("https://open.larksuite.com/open-apis/auth/v3/tenant_access_token/internal")
            .json(&json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .send()
            .await
            .map_err(|e| format!("token request failed: {e}"))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("token response parse failed: {e}"))?;

        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!("token API error: {body}"));
        }

        let token = body
            .get("tenant_access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing tenant_access_token in response".to_string())?
            .to_string();

        let expire = body.get("expire").and_then(|v| v.as_u64()).unwrap_or(7200);

        cached.value = token.clone();
        cached.expires_at = std::time::Instant::now() + std::time::Duration::from_secs(expire);

        info!("refreshed lark bot tenant access token (expires in {expire}s)");
        Ok(token)
    }

    /// Sends an interactive card to a user identified by `email`.
    pub async fn send_dm(&self, email: &str, card: &LarkCard) -> Result<(), String> {
        let token = self.get_token().await?;

        let payload = json!({
            "receive_id": email,
            "msg_type": "interactive",
            "content": serde_json::to_string(card).unwrap_or_default(),
        });

        let resp = self
            .http
            .post("https://open.larksuite.com/open-apis/im/v1/messages?receive_id_type=email")
            .header("Authorization", format!("Bearer {token}"))
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("DM request failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if status.is_success() {
            let parsed: serde_json::Value =
                serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            let code = parsed.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
            if code != 0 {
                return Err(format!("DM API returned code {code}: {body}"));
            }
            info!("DM sent to {email}");
            Ok(())
        } else {
            Err(format!("DM request returned {status}: {body}"))
        }
    }
}
