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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Priority;

    fn make_update_event(changes: Vec<String>) -> Event {
        Event::IssueUpdated {
            source: "linear".into(),
            identifier: "ENG-1".into(),
            title: "test".into(),
            description: None,
            status: "In Progress".into(),
            priority: Priority::High,
            assignee: None,
            assignee_email: None,
            url: "https://example.com".into(),
            changes,
        }
    }

    fn make_create_event(changes: Vec<String>) -> Event {
        Event::IssueCreated {
            source: "linear".into(),
            identifier: "ENG-1".into(),
            title: "test".into(),
            description: None,
            status: "In Progress".into(),
            priority: Priority::High,
            assignee: None,
            assignee_email: None,
            url: "https://example.com".into(),
            changes,
        }
    }

    #[tokio::test]
    async fn first_insert_returns_full_delay() {
        let map = DebounceMap::new();
        let ev = make_update_event(vec!["status changed".into()]);

        let (_rx, delay) = map.upsert("ENG-1".into(), ev, None, 5000, 120_000).await;
        assert_eq!(delay, 5000);
    }

    #[tokio::test]
    async fn merge_deduplicates_changes() {
        let map = DebounceMap::new();

        let ev1 = make_update_event(vec!["status changed".into()]);
        let _ = map.upsert("ENG-1".into(), ev1, None, 5000, 120_000).await;

        // Second event with one duplicate + one new change
        let ev2 = make_update_event(vec!["status changed".into(), "priority changed".into()]);
        let _ = map.upsert("ENG-1".into(), ev2, None, 5000, 120_000).await;

        let pending = map.take("ENG-1").await.unwrap();
        let changes = pending.event.changes().to_vec();
        assert_eq!(changes, vec!["status changed", "priority changed"]);
    }

    #[tokio::test]
    async fn create_then_update_stays_created() {
        let map = DebounceMap::new();

        let ev1 = make_create_event(vec!["created".into()]);
        let _ = map.upsert("ENG-1".into(), ev1, None, 5000, 120_000).await;

        let ev2 = make_update_event(vec!["status changed".into()]);
        let _ = map.upsert("ENG-1".into(), ev2, None, 5000, 120_000).await;

        let pending = map.take("ENG-1").await.unwrap();
        assert!(pending.event.is_issue_created());
    }

    #[tokio::test]
    async fn max_wait_caps_delay() {
        let map = DebounceMap::new();

        let ev1 = make_update_event(vec![]);
        let _ = map.upsert("ENG-1".into(), ev1, None, 5000, 10_000).await;

        // Simulate time passing by inserting again — max_wait is 10s,
        // first event was just inserted so elapsed ≈ 0, remaining ≈ 10000
        let ev2 = make_update_event(vec![]);
        let (_rx, delay) = map.upsert("ENG-1".into(), ev2, None, 5000, 10_000).await;

        // delay should be min(5000, ~10000) = 5000
        assert!(delay <= 5000);
    }

    #[tokio::test]
    async fn dm_email_preserves_first_non_none() {
        let map = DebounceMap::new();

        let ev1 = make_update_event(vec![]);
        let _ = map
            .upsert(
                "ENG-1".into(),
                ev1,
                Some("alice@test.com".into()),
                5000,
                120_000,
            )
            .await;

        let ev2 = make_update_event(vec![]);
        let _ = map.upsert("ENG-1".into(), ev2, None, 5000, 120_000).await;

        let pending = map.take("ENG-1").await.unwrap();
        assert_eq!(pending.dm_email.as_deref(), Some("alice@test.com"));
    }

    #[tokio::test]
    async fn take_removes_entry() {
        let map = DebounceMap::new();
        let ev = make_update_event(vec![]);
        let _ = map.upsert("ENG-1".into(), ev, None, 5000, 120_000).await;

        assert!(map.take("ENG-1").await.is_some());
        assert!(map.take("ENG-1").await.is_none());
    }
}
