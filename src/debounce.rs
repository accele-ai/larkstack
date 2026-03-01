//! Coalesces rapid-fire events on the same entity into a single notification.

use std::collections::HashMap;

use tokio::sync::{Mutex, oneshot};

use crate::event::Event;

/// A pending notification waiting for the debounce window to expire.
pub struct PendingUpdate {
    /// The latest event state (replaced on every new update).
    pub event: Event,
    /// Email to DM if any update in the window changed the assignee.
    pub dm_email: Option<String>,
    /// Send on this to cancel the currently-scheduled timer task.
    cancel_tx: oneshot::Sender<()>,
}

/// Thread-safe map of entity keys to their pending debounced updates.
pub struct DebounceMap(Mutex<HashMap<String, PendingUpdate>>);

impl Default for DebounceMap {
    fn default() -> Self {
        Self::new()
    }
}

impl DebounceMap {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }

    /// Inserts or merges an update for `key`.
    ///
    /// When an entry already exists the old timer is cancelled, change
    /// descriptions are merged (deduplicating exact matches), and the event
    /// is replaced with the latest state. A create followed by updates stays
    /// a create.
    ///
    /// Returns a [`oneshot::Receiver`] the caller should `select!` against a
    /// sleep — if it fires, a newer update has taken over.
    pub async fn upsert(
        &self,
        key: String,
        event: Event,
        dm_email: Option<String>,
    ) -> oneshot::Receiver<()> {
        let mut map = self.0.lock().await;

        let (merged_event, merged_dm_email) = if let Some(existing) = map.remove(&key) {
            let _ = existing.cancel_tx.send(());

            // Accumulate change descriptions; skip exact duplicates.
            let mut all: Vec<String> = existing.event.changes().to_vec();
            for c in event.changes() {
                if !all.contains(c) {
                    all.push(c.clone());
                }
            }

            // A create followed by updates is still a "create".
            let mut merged = if existing.event.is_issue_created() {
                event.promote_to_created()
            } else {
                event
            };
            merged.set_changes(all);

            (merged, dm_email.or(existing.dm_email))
        } else {
            (event, dm_email)
        };

        let (cancel_tx, cancel_rx) = oneshot::channel();
        map.insert(
            key,
            PendingUpdate {
                event: merged_event,
                dm_email: merged_dm_email,
                cancel_tx,
            },
        );
        cancel_rx
    }

    /// Removes and returns the pending update for `key`, if any.
    pub async fn take(&self, key: &str) -> Option<PendingUpdate> {
        self.0.lock().await.remove(key)
    }
}
