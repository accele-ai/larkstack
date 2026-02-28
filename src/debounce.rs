use std::collections::HashMap;

use tokio::sync::{oneshot, Mutex};

use crate::models::Issue;

pub struct PendingUpdate {
    /// Whether this debounce batch originated from a create event.
    pub is_create: bool,
    /// Latest issue state (replaced on every new update for this issue).
    pub issue: Issue,
    pub url: String,
    /// Accumulated change descriptions from all coalesced updates.
    pub changes: Vec<String>,
    /// Email to DM if any update in the window changed the assignee.
    pub dm_email: Option<String>,
    /// Send on this to cancel the currently-scheduled timer task.
    cancel_tx: oneshot::Sender<()>,
}

pub struct DebounceMap(Mutex<HashMap<String, PendingUpdate>>);

impl DebounceMap {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }

    /// Insert or merge an update for the given issue.
    ///
    /// If there is already a pending update for this issue, the old timer task
    /// is cancelled, the change descriptions are merged (deduplicating exact
    /// matches), and the issue state is replaced with the latest.
    ///
    /// Returns a `oneshot::Receiver` the caller should `select!` against a
    /// sleep — if it fires, a newer update has taken over.
    pub async fn upsert(
        &self,
        issue_id: String,
        issue: Issue,
        url: String,
        changes: Vec<String>,
        dm_email: Option<String>,
        is_create: bool,
    ) -> oneshot::Receiver<()> {
        let mut map = self.0.lock().await;

        let (merged_is_create, merged_changes, merged_dm_email) =
            if let Some(existing) = map.remove(&issue_id) {
                // Cancel the old timer task.
                let _ = existing.cancel_tx.send(());
                // A create followed by updates is still a "create".
                let merged_create = existing.is_create || is_create;
                // Accumulate change descriptions; skip exact duplicates.
                let mut all = existing.changes;
                for c in &changes {
                    if !all.contains(c) {
                        all.push(c.clone());
                    }
                }
                // Prefer the latest DM email if present.
                (merged_create, all, dm_email.or(existing.dm_email))
            } else {
                (is_create, changes, dm_email)
            };

        let (cancel_tx, cancel_rx) = oneshot::channel();
        map.insert(
            issue_id,
            PendingUpdate {
                is_create: merged_is_create,
                issue,
                url,
                changes: merged_changes,
                dm_email: merged_dm_email,
                cancel_tx,
            },
        );
        cancel_rx
    }

    /// Remove and return the pending update for the given issue, if any.
    pub async fn take(&self, issue_id: &str) -> Option<PendingUpdate> {
        self.0.lock().await.remove(issue_id)
    }
}
