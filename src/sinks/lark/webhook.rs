//! Sends card messages to the Lark group webhook.

use tracing::{error, info};

use crate::config::AppState;

use super::models::LarkMessage;

/// POSTs a card message to the configured Lark webhook URL.
pub async fn send_lark_card(state: &AppState, card: &LarkMessage) {
    match state
        .http
        .post(&state.lark.webhook_url)
        .json(card)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                info!("lark notification sent: {text}");
            } else {
                error!("lark returned {status}: {text}");
            }
        }
        Err(e) => {
            error!("failed to send lark notification: {e}");
        }
    }
}
