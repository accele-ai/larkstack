mod lark_event;
mod webhook;

pub use lark_event::lark_event_handler;
pub use webhook::webhook_handler;

use tracing::{error, info};

use crate::{
    config::AppState,
    lark::build_assign_dm_card,
    models::{Issue, LarkMessage},
};

// ---------------------------------------------------------------------------
// Health-check
// ---------------------------------------------------------------------------

pub async fn health() -> &'static str {
    "ok"
}

// ---------------------------------------------------------------------------
// Shared helpers (used by both webhook and lark_event sub-modules)
// ---------------------------------------------------------------------------

/// Send a card message to the configured Lark group webhook.
async fn send_lark_card(state: &AppState, card: &LarkMessage) {
    match state
        .http
        .post(&state.lark_webhook_url)
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

/// DM the assignee (graceful degradation — no-op if bot is not configured).
async fn try_dm_assignee(state: &AppState, issue: &Issue, url: &str, email: &str) {
    let Some(ref bot) = state.lark_bot else {
        return;
    };

    let card = build_assign_dm_card(issue, url);
    if let Err(e) = bot.send_dm(email, &card).await {
        error!("failed to DM assignee {email}: {e}");
    }
}
