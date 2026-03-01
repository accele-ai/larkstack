//! Axum handler for `POST /webhook` — receives Linear webhook payloads,
//! converts them to [`Event`]s, and feeds them through debounce / dispatch.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use tracing::{error, info, warn};

use crate::{
    config::AppState,
    dispatch,
    event::{Event, Priority},
};

#[cfg(not(feature = "cf-worker"))]
use crate::debounce::PendingUpdate;

use super::{
    models::{Actor, CommentData, Issue, LinearPayload, UpdatedFrom},
    utils::{build_change_fields, verify_signature},
};

/// Handles incoming Linear webhook requests.
///
/// 1. Verifies the `linear-signature` HMAC header.
/// 2. Deserializes the [`LinearPayload`].
/// 3. Converts to an [`Event`] and either debounces (issues) or dispatches
///    immediately (comments).
pub async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let signature = match headers
        .get("linear-signature")
        .and_then(|v| v.to_str().ok())
    {
        Some(s) => s,
        None => {
            warn!("missing linear-signature header");
            return StatusCode::UNAUTHORIZED;
        }
    };

    if !verify_signature(&state.linear.webhook_secret, &body, signature) {
        warn!("invalid webhook signature");
        return StatusCode::UNAUTHORIZED;
    }

    let payload: LinearPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            error!("failed to parse payload: {e}");
            return StatusCode::BAD_REQUEST;
        }
    };

    match (payload.kind.as_str(), payload.action.as_str()) {
        ("Issue", "create") => {
            let issue: Issue = match serde_json::from_value(payload.data.clone()) {
                Ok(i) => i,
                Err(e) => {
                    error!("failed to parse Issue data: {e}");
                    return StatusCode::BAD_REQUEST;
                }
            };
            info!(
                "queuing debounced Issue create – {} {}",
                issue.identifier, issue.title
            );

            let dm_email = issue.assignee.as_ref().and_then(|a| a.email.clone());
            let issue_id = issue.id.clone();

            let event = issue_to_event(&issue, &payload.url, vec![], true);

            schedule_debounce(&state, issue_id, event, dm_email).await;

            StatusCode::OK
        }
        ("Issue", "update") => {
            let issue: Issue = match serde_json::from_value(payload.data.clone()) {
                Ok(i) => i,
                Err(e) => {
                    error!("failed to parse Issue data: {e}");
                    return StatusCode::BAD_REQUEST;
                }
            };

            let changes = build_change_fields(&issue, &payload.updated_from);

            info!(
                "queuing debounced Issue update – {} {} (changes: {})",
                issue.identifier,
                issue.title,
                if changes.is_empty() {
                    "none detected".to_string()
                } else {
                    changes.join(", ")
                }
            );

            let dm_email: Option<String> = payload.updated_from.as_ref().and_then(|uf| {
                serde_json::from_value::<UpdatedFrom>(uf.clone())
                    .ok()
                    .and_then(|uf| {
                        if uf.assignee_id.is_some() {
                            issue.assignee.as_ref().and_then(|a| a.email.clone())
                        } else {
                            None
                        }
                    })
            });

            let issue_id = issue.id.clone();

            let event = issue_to_event(&issue, &payload.url, changes, false);

            schedule_debounce(&state, issue_id, event, dm_email).await;

            StatusCode::OK
        }
        ("Comment", "create") => {
            let comment: CommentData = match serde_json::from_value(payload.data.clone()) {
                Ok(c) => c,
                Err(e) => {
                    error!("failed to parse Comment data: {e}");
                    return StatusCode::BAD_REQUEST;
                }
            };

            let actor: Option<Actor> = payload
                .data
                .get("user")
                .and_then(|u| serde_json::from_value(u.clone()).ok());

            let identifier = comment
                .issue
                .as_ref()
                .map(|i| i.identifier.clone())
                .unwrap_or_else(|| "?".into());
            let issue_title = comment
                .issue
                .as_ref()
                .map(|i| i.title.clone())
                .unwrap_or_default();
            let author = actor.map(|a| a.name).unwrap_or_else(|| "Someone".into());

            info!("processing Comment create on {identifier}");

            let event = Event::CommentCreated {
                source: "linear".into(),
                identifier,
                issue_title,
                author,
                body: comment.body,
                url: payload.url,
            };

            dispatch::dispatch(&event, &state, None).await;
            StatusCode::OK
        }
        _ => {
            info!(
                "ignoring event: type={}, action={}",
                payload.kind, payload.action
            );
            StatusCode::OK
        }
    }
}

#[cfg(not(feature = "cf-worker"))]
async fn schedule_debounce(
    state: &Arc<AppState>,
    issue_id: String,
    event: Event,
    dm_email: Option<String>,
) {
    let cancel_rx = state
        .update_debounce
        .upsert(issue_id.clone(), event, dm_email)
        .await;

    let state2 = Arc::clone(state);
    let delay = state.server.debounce_delay_ms;
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {
                if let Some(p) = state2.update_debounce.take(&issue_id).await {
                    send_debounced_notification(&state2, p).await;
                }
            }
            _ = cancel_rx => {}
        }
    });
}

#[cfg(feature = "cf-worker")]
async fn schedule_debounce(
    state: &Arc<AppState>,
    issue_id: String,
    event: Event,
    dm_email: Option<String>,
) {
    let payload = serde_json::json!({
        "event": event,
        "dm_email": dm_email,
        "delay_ms": state.server.debounce_delay_ms,
    });

    let body = serde_json::to_string(&payload).unwrap();

    let stub = match state.env.durable_object("DEBOUNCER") {
        Ok(ns) => match ns.get_by_name(&issue_id) {
            Ok(stub) => stub,
            Err(e) => {
                error!("failed to get DO stub: {e}");
                return;
            }
        },
        Err(e) => {
            error!("DEBOUNCER binding not found: {e}");
            return;
        }
    };

    let mut init = worker::RequestInit::new();
    init.with_method(worker::Method::Post)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body)));

    let req = match worker::Request::new_with_init("https://do/schedule", &init) {
        Ok(r) => r,
        Err(e) => {
            error!("failed to build DO request: {e}");
            return;
        }
    };

    if let Err(e) = stub.fetch_with_request(req).await {
        error!("failed to schedule debounce via DO: {e}");
    }
}

/// Converts a Linear [`Issue`] into an [`Event`].
fn issue_to_event(issue: &Issue, url: &str, changes: Vec<String>, is_create: bool) -> Event {
    let fields = (
        "linear".to_string(),
        issue.identifier.clone(),
        issue.title.clone(),
        issue.description.clone(),
        issue.state.name.clone(),
        Priority::from_linear(issue.priority),
        issue.assignee.as_ref().map(|a| a.name.clone()),
        issue.assignee.as_ref().and_then(|a| a.email.clone()),
        url.to_string(),
        changes,
    );

    if is_create {
        Event::IssueCreated {
            source: fields.0,
            identifier: fields.1,
            title: fields.2,
            description: fields.3,
            status: fields.4,
            priority: fields.5,
            assignee: fields.6,
            assignee_email: fields.7,
            url: fields.8,
            changes: fields.9,
        }
    } else {
        Event::IssueUpdated {
            source: fields.0,
            identifier: fields.1,
            title: fields.2,
            description: fields.3,
            status: fields.4,
            priority: fields.5,
            assignee: fields.6,
            assignee_email: fields.7,
            url: fields.8,
            changes: fields.9,
        }
    }
}

#[cfg(not(feature = "cf-worker"))]
async fn send_debounced_notification(state: &AppState, pending: PendingUpdate) {
    let kind = if pending.event.is_issue_created() {
        "create"
    } else {
        "update"
    };
    let changes = pending.event.changes();
    let changes_str = if changes.is_empty() {
        "none".to_string()
    } else {
        changes.join(", ")
    };

    info!("sending debounced {kind} – changes: {changes_str}");

    dispatch::dispatch(&pending.event, state, pending.dm_email.as_deref()).await;
}
