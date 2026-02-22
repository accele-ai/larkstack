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
                "processing Issue update – {} {} (changes: {})",
                issue.identifier,
                issue.title,
                if changes.is_empty() {
                    "none detected".to_string()
                } else {
                    changes.join(", ")
                }
            );

            let card_msg = build_lark_card(&CardEvent::IssueUpdated {
                issue: &issue,
                url: &payload.url,
                changes: changes.clone(),
            });

            // Phase 2: DM new assignee if assignee changed
            if let Some(ref uf) = payload.updated_from {
                let uf_parsed: Result<UpdatedFrom, _> = serde_json::from_value(uf.clone());
                if let Ok(updated_from) = uf_parsed {
                    if updated_from.assignee_id.is_some() {
                        if let Some(ref assignee) = issue.assignee {
                            if let Some(ref email) = assignee.email {
                                try_dm_assignee(&state, &issue, &payload.url, email).await;
                            }
                        }
                    }
                }
            }

            card_msg
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
            let card = build_preview_card(&issue);
            // url.preview.get expects {"card": { header, elements }}
            (StatusCode::OK, Json(serde_json::json!({ "card": card })))
        }
        Err(e) => {
            error!("failed to fetch Linear issue {identifier}: {e}");
            (StatusCode::OK, Json(serde_json::json!({})))
        }
    }
}

// ---------------------------------------------------------------------------
// Health-check
// ---------------------------------------------------------------------------

pub async fn health() -> &'static str {
    "ok"
}
