//! Durable Object-based debounce for Cloudflare Workers.
//!
//! Replaces the in-memory [`DebounceMap`](crate::debounce::DebounceMap) with a
//! Durable Object that uses alarms to coalesce rapid-fire events.

use std::time::Duration;

use worker::*;

use crate::event::Event;

#[durable_object]
pub struct DebounceObject {
    state: State,
    env: Env,
}

impl DurableObject for DebounceObject {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let body: serde_json::Value = serde_json::from_str(
            &req.text()
                .await
                .map_err(|e| Error::RustError(format!("read body: {e}")))?,
        )
        .map_err(|e| Error::RustError(format!("parse json: {e}")))?;

        let event: Event = serde_json::from_value(body["event"].clone())
            .map_err(|e| Error::RustError(format!("parse event: {e}")))?;
        let dm_email: Option<String> =
            serde_json::from_value(body["dm_email"].clone()).unwrap_or(None);
        let delay_ms: u64 = serde_json::from_value(body["delay_ms"].clone())
            .map_err(|e| Error::RustError(format!("parse delay: {e}")))?;
        let max_wait_ms: u64 = body["max_wait_ms"].as_u64().unwrap_or(120_000);

        let storage = self.state.storage();

        // Merge with existing event if any (same logic as DebounceMap::upsert).
        let (merged_event, merged_dm_email) =
            if let Some(existing) = storage.get::<Event>("event").await? {
                let mut all: Vec<String> = existing.changes().to_vec();
                for c in event.changes() {
                    if !all.contains(c) {
                        all.push(c.clone());
                    }
                }

                let mut merged = if existing.is_issue_created() {
                    event.promote_to_created()
                } else {
                    event
                };
                merged.set_changes(all);

                let existing_dm: Option<String> =
                    storage.get::<String>("dm_email").await.unwrap_or(None);
                (merged, dm_email.or(existing_dm))
            } else {
                (event, dm_email)
            };

        storage.put("event", &merged_event).await?;
        if let Some(ref email) = merged_dm_email {
            storage.put("dm_email", email).await?;
        }

        // Track when the first event in this window arrived for max_wait enforcement.
        let now_ms = js_sys::Date::now() as u64;
        let first_received_ms: u64 = storage
            .get::<u64>("first_received_ms")
            .await
            .unwrap_or(None)
            .unwrap_or(now_ms);
        storage.put("first_received_ms", &first_received_ms).await?;

        // Respect max_wait: never delay longer than max_wait_ms from the first event.
        let elapsed_ms = now_ms.saturating_sub(first_received_ms);
        let remaining_max = max_wait_ms.saturating_sub(elapsed_ms);
        let actual_delay = delay_ms.min(remaining_max).max(1);

        // Schedule (or reschedule) the alarm.
        storage
            .set_alarm(Duration::from_millis(actual_delay))
            .await?;

        Response::ok("scheduled")
    }

    async fn alarm(&self) -> Result<Response> {
        let storage = self.state.storage();

        let event: Event = storage
            .get("event")
            .await?
            .ok_or_else(|| Error::RustError("alarm: no event in storage".into()))?;
        let dm_email: Option<String> = storage.get::<String>("dm_email").await.unwrap_or(None);

        storage.delete_all().await?;

        let http = reqwest::Client::new();

        // Deliver to the Linear group chat via incoming webhook.
        let card = crate::sinks::lark::cards::build_lark_card(&event);
        let webhook_url = self
            .env
            .secret("LARK_WEBHOOK_URL")
            .map(|s| s.to_string())
            .unwrap_or_default();
        if !webhook_url.is_empty() {
            crate::sinks::lark::webhook::send_lark_card(&http, &webhook_url, &card).await;
        } else {
            worker::console_error!("LARK_WEBHOOK_URL not configured — group notification skipped");
        }

        // Send DM via the Linear DM bot (enterprise self-built app).
        if let Some(email) = &dm_email {
            let app_id = self.env.secret("LARK_APP_ID").ok().map(|s| s.to_string());
            let app_secret = self
                .env
                .secret("LARK_APP_SECRET")
                .ok()
                .map(|s| s.to_string());
            if let (Some(id), Some(secret)) = (app_id, app_secret) {
                let bot = crate::sinks::lark::LarkBotClient::new(id, secret, http.clone());
                crate::sinks::lark::try_dm(&event, &bot, email).await;
            }
        }

        Response::ok("dispatched")
    }
}
