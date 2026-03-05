//! Routes an [`Event`] to every registered sink.

use crate::{config::AppState, event::Event, sinks};

/// Sends `event` to the Linear Lark group. If `dm_email` is provided, a
/// direct message is also sent using the Linear bot credentials.
pub async fn dispatch(event: &Event, state: &AppState, dm_email: Option<&str>) {
    sinks::lark::notify(event, state).await;

    if let (Some(email), Some(bot)) = (dm_email, &state.lark_bot) {
        sinks::lark::try_dm(event, bot, email).await;
    }
}

/// Sends `event` to the GitHub Lark group. If `dm_email` is provided, a
/// direct message is sent using the GitHub bot credentials (falling back to
/// the Linear bot when no GitHub-specific bot is configured).
pub async fn dispatch_github(event: &Event, state: &AppState, dm_email: Option<&str>) {
    sinks::lark::notify_github(event, state).await;

    let bot = state.github_lark_bot.as_ref().or(state.lark_bot.as_ref());
    if let (Some(email), Some(bot)) = (dm_email, bot) {
        sinks::lark::try_dm(event, bot, email).await;
    }
}
