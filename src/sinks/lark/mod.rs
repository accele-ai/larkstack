//! Lark (Feishu) notification sink — group webhook cards and bot DMs.

mod bot;
pub mod cards;
pub mod event_handler;
pub mod models;
pub(crate) mod webhook;

pub use bot::LarkBotClient;
pub use event_handler::lark_event_handler;

use tracing::error;

use crate::{config::AppState, event::Event};

/// Sends a card notification for `event` to the Linear group chat via webhook.
pub async fn notify(event: &Event, state: &AppState) {
    let card = cards::build_lark_card(event);
    if !state.lark.webhook_url.is_empty() {
        webhook::send_lark_card(&state.http, &state.lark.webhook_url, &card).await;
    } else {
        error!("LARK_WEBHOOK_URL not configured — Linear group chat notification skipped");
    }
}

/// Sends a card notification for `event` to the GitHub group chat via webhook.
///
/// Uses `LARK_GITHUB_WEBHOOK_URL` when set, falls back to `LARK_WEBHOOK_URL`.
pub async fn notify_github(event: &Event, state: &AppState) {
    let card = cards::build_lark_card(event);
    let webhook = if !state.lark.github_webhook_url.is_empty() {
        &state.lark.github_webhook_url
    } else {
        &state.lark.webhook_url
    };
    if !webhook.is_empty() {
        webhook::send_lark_card(&state.http, webhook, &card).await;
    } else {
        error!("no webhook URL configured — GitHub group chat notification skipped");
    }
}

/// Sends a DM about `event` via the enterprise self-built app bot.
/// No-op when the event does not support DM notifications.
pub async fn try_dm(event: &Event, bot: &LarkBotClient, email: &str) {
    if let Some(card) = cards::build_assign_dm_card(event)
        && let Err(e) = bot.send_dm(email, &card).await
    {
        error!("failed to DM {email}: {e}");
    }
}
