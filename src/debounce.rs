//! Coalesces rapid-fire events on the same entity into a single notification.

use std::collections::HashMap;
use std::time::Instant;

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
    /// When the first event in this debounce window arrived.
    first_received_at: Instant,
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
    /// Returns `(cancel_rx, actual_delay_ms)`. The caller should `select!`
    /// the cancel_rx against a sleep of `actual_delay_ms`. The actual delay
    /// is `min(delay_ms, remaining_max_wait)` so the window never exceeds
    /// `max_wait_ms` from the first event.
    pub async fn upsert(
        &self,
        key: String,
        event: Event,
        dm_email: Option<String>,
        delay_ms: u64,
        max_wait_ms: u64,
    ) -> (oneshot::Receiver<()>, u64) {
        let mut map = self.0.lock().await;

        let (merged_event, merged_dm_email, first_received_at) =
            if let Some(existing) = map.remove(&key) {
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

                (
                    merged,
                    dm_email.or(existing.dm_email),
                    existing.first_received_at,
                )
            } else {
                (event, dm_email, Instant::now())
            };

        // Respect max_wait: never delay longer than max_wait_ms from the first event.
        let elapsed_ms = first_received_at.elapsed().as_millis() as u64;
        let remaining_max = max_wait_ms.saturating_sub(elapsed_ms);
        let actual_delay = delay_ms.min(remaining_max).max(1);

        let (cancel_tx, cancel_rx) = oneshot::channel();
        map.insert(
            key,
            PendingUpdate {
                event: merged_event,
                dm_email: merged_dm_email,
                cancel_tx,
                first_received_at,
            },
        );
        (cancel_rx, actual_delay)
    }

    /// Removes and returns the pending update for `key`, if any.
    pub async fn take(&self, key: &str) -> Option<PendingUpdate> {
        self.0.lock().await.remove(key)
    }
}
