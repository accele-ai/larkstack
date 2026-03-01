//! Lark (Feishu) notification sink — group webhook cards and bot DMs.

mod bot;
pub mod cards;
pub mod event_handler;
pub mod models;
mod webhook;

pub use bot::LarkBotClient;
pub use event_handler::lark_event_handler;

use tracing::error;

use crate::{config::AppState, event::Event};

/// Sends a card notification for `event` to the configured Lark group webhook.
pub async fn notify(event: &Event, state: &AppState) {
    let card = cards::build_lark_card(event);
    webhook::send_lark_card(state, &card).await;
}

/// DMs the assignee about `event` (no-op when the bot is not configured).
pub async fn try_dm(event: &Event, state: &AppState, email: &str) {
    let Some(ref bot) = state.lark_bot else {
        return;
    };

    let card = cards::build_assign_dm_card(event);
    if let Err(e) = bot.send_dm(email, &card).await {
        error!("failed to DM assignee {email}: {e}");
    }
}
