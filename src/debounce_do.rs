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

        // Schedule (or reschedule) the alarm.
        storage.set_alarm(Duration::from_millis(delay_ms)).await?;

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

        // Build the card once, then deliver via Bot API or webhook fallback.
        let card = crate::sinks::lark::cards::build_lark_card(&event);

        let app_id = self.env.var("LARK_APP_ID").ok().map(|v| v.to_string());
        let app_secret = self
            .env
            .secret("LARK_APP_SECRET")
            .ok()
            .map(|s| s.to_string());
        let target_chat_id = self
            .env
            .var("LARK_TARGET_CHAT_ID")
            .ok()
            .map(|v| v.to_string());

        let bot = match (app_id, app_secret) {
            (Some(id), Some(secret)) => Some(crate::sinks::lark::LarkBotClient::new(
                id,
                secret,
                http.clone(),
            )),
            _ => None,
        };

        match (&bot, &target_chat_id) {
            (Some(b), Some(chat_id)) => {
                if let Err(e) = b.send_to_chat(chat_id, &card.card).await {
                    worker::console_error!("failed to send card to chat: {e}");
                }
            }
            _ => {
                let webhook_url = self
                    .env
                    .var("LARK_WEBHOOK_URL")
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                if !webhook_url.is_empty() {
                    crate::sinks::lark::webhook::send_lark_card(&http, &webhook_url, &card).await;
                }
            }
        }

        if let (Some(ref email), Some(ref b)) = (&dm_email, &bot) {
            crate::sinks::lark::try_dm(&event, b, email).await;
        }

        Response::ok("dispatched")
    }
}
