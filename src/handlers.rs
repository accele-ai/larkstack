use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use tracing::{error, info, warn};

use crate::{
    config::AppState,
    debounce::{PendingUpdate, DEBOUNCE_MS},
    lark::{build_assign_dm_card, build_lark_card, CardEvent},
    linear::{build_preview_card, extract_identifier_from_url},
    models::{Actor, CommentData, Issue, LinearPayload, UpdatedFrom},
    utils::{build_change_fields, verify_signature},
};

// ---------------------------------------------------------------------------
// Webhook handler
// ---------------------------------------------------------------------------

pub async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    // 1. Signature verification
    let signature = match headers.get("linear-signature").and_then(|v| v.to_str().ok()) {
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
                "processing Issue create – {} {}",
                issue.identifier, issue.title
            );

            let card_msg = build_lark_card(&CardEvent::IssueCreated {
                issue: &issue,
                url: &payload.url,
            });

            // Phase 2: DM assignee on create if assignee is set
            if let Some(ref assignee) = issue.assignee {
                if let Some(ref email) = assignee.email {
                    try_dm_assignee(&state, &issue, &payload.url, email).await;
                }
            }

            card_msg
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

            // Merge with any pending update for this issue and (re)start the timer.
            let cancel_rx = {
                let mut map = state.update_debounce.0.lock().await;

                let (merged_changes, merged_dm_email) =
                    if let Some(existing) = map.remove(&issue_id) {
                        // Cancel the old timer task.
                        let _ = existing.cancel_tx.send(());
                        // Accumulate change descriptions; skip exact duplicates.
                        let mut all = existing.changes;
                        for c in &changes {
                            if !all.contains(c) {
                                all.push(c.clone());
                            }
                        }
                        // Prefer the latest DM email if present.
                        (all, dm_email.or(existing.dm_email))
                    } else {
                        (changes, dm_email)
                    };

                let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
                map.insert(
                    issue_id.clone(),
                    PendingUpdate {
                        issue,
                        url: payload.url.clone(),
                        changes: merged_changes,
                        dm_email: merged_dm_email,
                        cancel_tx,
                    },
                );
                cancel_rx
            };

            // Spawn the timer task; whichever fires first wins.
            let state2 = Arc::clone(&state);
            tokio::spawn(async move {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(DEBOUNCE_MS)) => {
                        let pending = state2.update_debounce.0.lock().await.remove(&issue_id);
                        if let Some(p) = pending {
                            send_update_notification(&state2, p).await;
                        }
                    }
                    _ = cancel_rx => {
                        // A newer update cancelled this task; the replacement task will fire.
                    }
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
    match state
        .http
        .post(&state.lark_webhook_url)
        .json(&card)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                info!("lark notification sent: {text}");
            } else {
                error!("lark returned {status}: {text}");
            }
        }
        Err(e) => {
            error!("failed to send lark notification: {e}");
        }
    }

    StatusCode::OK
}

// ---------------------------------------------------------------------------
// Debounced update sender
// ---------------------------------------------------------------------------

async fn send_update_notification(state: &AppState, pending: PendingUpdate) {
    let PendingUpdate { issue, url, changes, dm_email, .. } = pending;

    info!(
        "sending debounced update for {} – changes: {}",
        issue.identifier,
        if changes.is_empty() { "none".to_string() } else { changes.join(", ") }
    );

    let card_msg = build_lark_card(&CardEvent::IssueUpdated {
        issue: &issue,
        url: &url,
        changes,
    });

    match state.http.post(&state.lark_webhook_url).json(&card_msg).send().await {
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                info!("lark update notification sent: {text}");
            } else {
                error!("lark returned {status}: {text}");
            }
        }
        Err(e) => error!("failed to send lark notification: {e}"),
    }

    if let Some(ref email) = dm_email {
        try_dm_assignee(state, &issue, &url, email).await;
    }
}

// ---------------------------------------------------------------------------
// Phase 2: DM helper (graceful degradation)
// ---------------------------------------------------------------------------

async fn try_dm_assignee(state: &AppState, issue: &Issue, url: &str, email: &str) {
    let Some(ref bot) = state.lark_bot else {
        return;
    };

    let card = build_assign_dm_card(issue, url);
    if let Err(e) = bot.send_dm(email, &card).await {
        error!("failed to DM assignee {email}: {e}");
    }
}

// ---------------------------------------------------------------------------
// Phase 3: Lark event handler (link preview / unfurl)
// ---------------------------------------------------------------------------

pub async fn lark_event_handler(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    let body_value: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            error!("failed to parse lark event body: {e}");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid json"})),
            );
        }
    };

    // Challenge verification
    if body_value.get("type").and_then(|v| v.as_str()) == Some("url_verification") {
        let challenge = body_value
            .get("challenge")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        info!("lark challenge verification");
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "challenge": challenge })),
        );
    }

    // Verify token if configured
    if let Some(ref expected_token) = state.lark_verification_token {
        let token = body_value
            .get("header")
            .and_then(|h| h.get("token"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if token != expected_token {
            warn!("lark event token mismatch");
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid token"})),
            );
        }
    }

    // Log full event body so we can inspect event_type and structure
    info!("lark event received: {body_value}");

    // Handle URL preview event
    let event_type = body_value
        .get("header")
        .and_then(|h| h.get("event_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if event_type == "url.preview.get" {
        return handle_link_preview(&state, &body_value).await;
    }

    info!("ignoring lark event type: '{event_type}' – add handler if needed");
    (StatusCode::OK, Json(serde_json::json!({})))
}

async fn handle_link_preview(
    state: &AppState,
    body: &serde_json::Value,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(ref linear) = state.linear_client else {
        warn!("link preview requested but LINEAR_API_KEY not configured");
        return (StatusCode::OK, Json(serde_json::json!({})));
    };

    // Extract the URL from the event
    let url = body
        .get("event")
        .and_then(|e| e.get("url"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            // Some Lark event formats nest it differently
            body.get("event")
                .and_then(|e| e.get("body"))
                .and_then(|b| b.get("url"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("");

    let Some(identifier) = extract_identifier_from_url(url) else {
        info!("could not extract Linear identifier from URL: {url}");
        return (StatusCode::OK, Json(serde_json::json!({})));
    };

    info!("fetching Linear issue {identifier} for link preview");

    match linear.fetch_issue_by_identifier(&identifier).await {
        Ok(issue) => {
            let inline_title = format!("[{}] {}", issue.identifier, issue.title);
            let card = build_preview_card(&issue);
            info!("built preview card for {identifier}: {inline_title}");
            // url.preview.get response format (per Lark internal docs):
            //   inline  – required text-link preview
            //   card    – optional card preview; type "raw" = full card JSON in data
            (StatusCode::OK, Json(serde_json::json!({
                "inline": {
                    "i18n_title": {
                        "en_us": inline_title,
                        "zh_cn": inline_title,
                    }
                },
                "card": {
                    "type": "raw",
                    "data": card
                }
            })))
        }
        Err(e) => {
            error!("failed to fetch Linear issue {identifier}: {e}");
            // inline is required even on error — return identifier as fallback
            (StatusCode::OK, Json(serde_json::json!({
                "inline": {
                    "i18n_title": { "en_us": identifier }
                }
            })))
        }
    }
}

// ---------------------------------------------------------------------------
// Health-check
// ---------------------------------------------------------------------------

pub async fn health() -> &'static str {
    "ok"
}
