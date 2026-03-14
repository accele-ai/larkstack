//! Axum handler for `POST /lark/event` — Lark platform callbacks including
//! challenge verification and URL link preview (unfurl).

use std::sync::Arc;

use axum::{Json, body::Bytes, extract::State, http::StatusCode};
use tracing::{error, info, warn};

use crate::{
    config::AppState,
    sources::{linear::client::extract_identifier_from_url, x::extract_tweet_id},
};

use super::cards::{build_preview_card, build_x_preview_card};

/// Decrypts a Lark AES-256-CBC encrypted payload.
///
/// Lark sends `{"encrypt": "<base64>"}` when an Encrypt Key is configured.
/// Key = SHA256(encrypt_key), IV = first 16 bytes of decoded data.
fn decrypt_lark_payload(
    encrypt_key: &str,
    encrypted_data: &str,
) -> Result<serde_json::Value, String> {
    use aes::Aes256;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;
    use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
    use sha2::{Digest, Sha256};

    type Aes256CbcDec = cbc::Decryptor<Aes256>;

    let key = Sha256::digest(encrypt_key.as_bytes());

    let encrypted_bytes = B64
        .decode(encrypted_data)
        .map_err(|e| format!("base64 decode: {e}"))?;

    if encrypted_bytes.len() < 16 {
        return Err("encrypted payload too short".into());
    }

    let (iv, ciphertext) = encrypted_bytes.split_at(16);

    let decryptor =
        Aes256CbcDec::new_from_slices(&key, iv).map_err(|e| format!("cipher init: {e}"))?;

    let mut buf = ciphertext.to_vec();
    let plaintext = decryptor
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| format!("decrypt: {e}"))?;

    serde_json::from_slice(plaintext).map_err(|e| format!("json parse: {e}"))
}

/// Handles incoming Lark event callbacks.
///
/// Supports `url_verification` challenges and `url.preview.get` link previews.
/// When `LARK_X_ENCRYPT_KEY` is set, AES-256-CBC encrypted payloads are decrypted
/// before processing.
pub async fn lark_event_handler(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    let raw: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            error!("failed to parse lark event body: {e}");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid json"})),
            );
        }
    };

    // Decrypt if Lark sent an encrypted payload (Encrypt Key is configured in the app).
    let body_value: serde_json::Value =
        if let Some(encrypted) = raw.get("encrypt").and_then(|v| v.as_str()) {
            match state.lark.x_encrypt_key.as_deref() {
                Some(key) => match decrypt_lark_payload(key, encrypted) {
                    Ok(v) => v,
                    Err(e) => {
                        error!("failed to decrypt lark payload: {e}");
                        return (
                            StatusCode::UNAUTHORIZED,
                            Json(serde_json::json!({"error": "decryption failed"})),
                        );
                    }
                },
                None => {
                    warn!("received encrypted lark payload but LARK_X_ENCRYPT_KEY is not set");
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({"error": "encrypt key not configured"})),
                    );
                }
            }
        } else {
            raw
        };

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

    let incoming_token = body_value
        .get("header")
        .and_then(|h| h.get("token"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let token_required =
        state.lark.verification_token.is_some() || state.lark.x_verification_token.is_some();

    if token_required {
        let valid = state
            .lark
            .verification_token
            .as_deref()
            .is_some_and(|t| t == incoming_token)
            || state
                .lark
                .x_verification_token
                .as_deref()
                .is_some_and(|t| t == incoming_token);

        if !valid {
            warn!("lark event token mismatch");
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid token"})),
            );
        }
    }

    let event_type_log = body_value
        .get("header")
        .and_then(|h| h.get("event_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    info!("lark event received: type={event_type_log}");

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

/// Handles `url.preview.get` — routes to X or Linear preview based on URL.
async fn handle_link_preview(
    state: &AppState,
    body: &serde_json::Value,
) -> (StatusCode, Json<serde_json::Value>) {
    let event = body.get("event");
    let url = event
        .and_then(|e| e.get("context"))
        .and_then(|c| c.get("url"))
        .and_then(|v| v.as_str())
        .or_else(|| event.and_then(|e| e.get("url")).and_then(|v| v.as_str()))
        .or_else(|| {
            event
                .and_then(|e| e.get("body"))
                .and_then(|b| b.get("url"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("");

    // X / Twitter link
    if let Some(tweet_id) = extract_tweet_id(url) {
        info!("fetching tweet {tweet_id} for link preview");
        let tweet = state.x_client.fetch(tweet_id, url).await;
        let (card, inline_title) = build_x_preview_card(&tweet);
        info!("built X preview card: {inline_title}");
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "inline": {
                    "i18n_title": {
                        "en_us": inline_title,
                        "zh_cn": inline_title,
                    }
                },
                "card": { "type": "raw", "data": card }
            })),
        );
    }

    // Linear link
    let Some(ref linear) = state.linear_client else {
        info!("link preview requested but no handler matched URL: {url}");
        return (StatusCode::OK, Json(serde_json::json!({})));
    };

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
                    "card": { "type": "raw", "data": card }
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
