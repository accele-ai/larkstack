use std::collections::HashMap;

use tokio::sync::{oneshot, Mutex};

use crate::models::Issue;

/// How long to wait after the last update before firing the notification.
/// Burst duplicates from Linear arrive within ~100ms of each other; 500ms
/// is comfortably above that while keeping notifications responsive.
pub const DEBOUNCE_MS: u64 = 500;

pub struct PendingUpdate {
    /// Latest issue state (replaced on every new update for this issue).
    pub issue: Issue,
    pub url: String,
    /// Accumulated change descriptions from all coalesced updates.
    pub changes: Vec<String>,
    /// Email to DM if any update in the window changed the assignee.
    pub dm_email: Option<String>,
    /// Send on this to cancel the currently-scheduled timer task.
    pub cancel_tx: oneshot::Sender<()>,
}

pub struct DebounceMap(pub Mutex<HashMap<String, PendingUpdate>>);

impl DebounceMap {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}
