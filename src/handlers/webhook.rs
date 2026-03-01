use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use tracing::{error, info, warn};

use crate::{
    config::AppState,
    debounce::PendingUpdate,
    lark::{build_lark_card, CardEvent},
    models::{Actor, CommentData, Issue, LinearPayload, UpdatedFrom},
    utils::{build_change_fields, verify_signature},
};

use super::send_lark_card;

// ---------------------------------------------------------------------------
// Webhook handler
// ---------------------------------------------------------------------------

pub async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    // 1. Signature verification
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

    if !verify_signature(&state.webhook_secret, &body, signature) {
        warn!("invalid webhook signature");
        return StatusCode::UNAUTHORIZED;
    }

    // 2. Deserialize payload
    let payload: LinearPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            error!("failed to parse payload: {e}");
            return StatusCode::BAD_REQUEST;
        }
    };

    // 3. Dispatch based on (kind, action)
    let card = match (payload.kind.as_str(), payload.action.as_str()) {
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

            let cancel_rx = state
                .update_debounce
                .upsert(
                    issue_id.clone(),
                    issue,
                    payload.url.clone(),
                    vec![],
                    dm_email,
                    true,
                )
                .await;

            let state2 = Arc::clone(&state);
            let delay = state.debounce_delay_ms;
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

            return StatusCode::OK;
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

            // Resolve DM email if the assignee changed in this update.
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

            let cancel_rx = state
                .update_debounce
                .upsert(
                    issue_id.clone(),
                    issue,
                    payload.url.clone(),
                    changes,
                    dm_email,
                    false,
                )
                .await;

            let state2 = Arc::clone(&state);
            let delay = state.debounce_delay_ms;
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

            return StatusCode::OK;
        }
        ("Comment", "create") => {
            let comment: CommentData = match serde_json::from_value(payload.data.clone()) {
                Ok(c) => c,
                Err(e) => {
                    error!("failed to parse Comment data: {e}");
                    return StatusCode::BAD_REQUEST;
                }
            };

            // Try to get actor from the top-level payload (Linear sends it sometimes)
            let actor: Option<Actor> = payload
                .data
                .get("user")
                .and_then(|u| serde_json::from_value(u.clone()).ok());

            let issue_ref = comment
                .issue
                .as_ref()
                .map(|i| i.identifier.as_str())
                .unwrap_or("?");
            info!("processing Comment create on {}", issue_ref);

            build_lark_card(&CardEvent::CommentCreated {
                comment: &comment,
                url: &payload.url,
                actor: actor.as_ref(),
            })
        }
        _ => {
            info!(
                "ignoring event: type={}, action={}",
                payload.kind, payload.action
            );
            return StatusCode::OK;
        }
    };

    // 4. Send Lark group card
    send_lark_card(&state, &card).await;
    StatusCode::OK
}

// ---------------------------------------------------------------------------
// Debounced notification sender (handles both create and update)
// ---------------------------------------------------------------------------

async fn send_debounced_notification(state: &AppState, pending: PendingUpdate) {
    let PendingUpdate {
        is_create,
        issue,
        url,
        changes,
        dm_email,
        ..
    } = pending;

    let kind = if is_create { "create" } else { "update" };
    info!(
        "sending debounced {kind} for {} – changes: {}",
        issue.identifier,
        if changes.is_empty() {
            "none".to_string()
        } else {
            changes.join(", ")
        }
    );

    let card_msg = if is_create {
        build_lark_card(&CardEvent::IssueCreated {
            issue: &issue,
            url: &url,
            changes,
        })
    } else {
        build_lark_card(&CardEvent::IssueUpdated {
            issue: &issue,
            url: &url,
            changes,
        })
    };

    send_lark_card(state, &card_msg).await;

    if let Some(ref email) = dm_email {
        super::try_dm_assignee(state, &issue, &url, email).await;
    }
}
