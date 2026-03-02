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

/// Sends a card notification for `event` to the Lark group.
///
/// Prefers Bot API (`target_chat_id`) when available, falls back to the
/// simple webhook (`webhook_url`).
pub async fn notify(event: &Event, state: &AppState) {
    let card = cards::build_lark_card(event);

    match (&state.lark_bot, &state.lark.target_chat_id) {
        (Some(bot), Some(chat_id)) => {
            if let Err(e) = bot.send_to_chat(chat_id, &card.card).await {
                error!("failed to send card to chat {chat_id}: {e}");
            }
        }
        _ if !state.lark.webhook_url.is_empty() => {
            webhook::send_lark_card(&state.http, &state.lark.webhook_url, &card).await;
        }
        _ => {
            error!(
                "no Lark delivery method configured (need LARK_TARGET_CHAT_ID + bot, or LARK_WEBHOOK_URL)"
            );
        }
    }
}

/// DMs the assignee about `event` (no-op when `bot` is `None` or event
/// does not support DM notifications).
pub async fn try_dm(event: &Event, bot: &LarkBotClient, email: &str) {
    if let Some(card) = cards::build_assign_dm_card(event)
        && let Err(e) = bot.send_dm(email, &card).await
    {
        error!("failed to DM {email}: {e}");
    }
}
