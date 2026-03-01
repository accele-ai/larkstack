//! Routes an [`Event`] to every registered sink.

use crate::{config::AppState, event::Event, sinks};

/// Sends `event` to all sinks. If `dm_email` is provided, a direct message
/// is also sent to that address.
pub async fn dispatch(event: &Event, state: &AppState, dm_email: Option<&str>) {
    sinks::lark::notify(event, &state.http, &state.lark.webhook_url).await;

    if let (Some(email), Some(bot)) = (dm_email, &state.lark_bot) {
        sinks::lark::try_dm(event, bot, email).await;
    }
}
