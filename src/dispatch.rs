//! Routes an [`Event`] to every registered sink.

use crate::{config::AppState, event::Event, sinks};

/// Sends `event` to all sinks. If `dm_email` is provided, a direct message
/// is also sent to that address.
pub async fn dispatch(event: &Event, state: &AppState, dm_email: Option<&str>) {
    sinks::lark::notify(event, state).await;

    if let Some(email) = dm_email {
        sinks::lark::try_dm(event, state, email).await;
    }
}
