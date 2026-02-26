use std::sync::Arc;

use axum::{body::Bytes, extract::State, http::StatusCode, Json};
use tracing::{error, info, warn};

use crate::{
    config::AppState,
    linear::{build_preview_card, extract_identifier_from_url},
};

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
            (
                StatusCode::OK,
                Json(serde_json::json!({
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
                })),
            )
        }
        Err(e) => {
            error!("failed to fetch Linear issue {identifier}: {e}");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "inline": {
                        "i18n_title": { "en_us": identifier }
                    }
                })),
            )
        }
    }
}
