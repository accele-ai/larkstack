//! Sends card messages to the Lark group webhook.

use reqwest::Client;
use tracing::{error, info};

use super::models::LarkMessage;

/// POSTs a card message to the given Lark webhook URL.
pub async fn send_lark_card(http: &Client, webhook_url: &str, card: &LarkMessage) {
    match http.post(webhook_url).json(card).send().await {
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
