use std::{env, sync::Arc};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// Config & shared state
// ---------------------------------------------------------------------------

struct AppState {
    webhook_secret: String,
    lark_webhook_url: String,
    http: Client,
    lark_bot: Option<LarkBotClient>,
    linear_client: Option<LinearClient>,
    lark_verification_token: Option<String>,
}

// ---------------------------------------------------------------------------
// Linear webhook models
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LinearPayload {
    action: String,
    #[serde(rename = "type")]
    kind: String,
    data: serde_json::Value,
    url: String,
    #[serde(rename = "updatedFrom")]
    updated_from: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Issue {
    #[allow(dead_code)]
    id: String,
    title: String,
    priority: u8,
    state: IssueState,
    assignee: Option<Assignee>,
    identifier: String,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IssueState {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Assignee {
    name: String,
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdatedFrom {
    #[serde(default)]
    state: Option<serde_json::Value>,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    assignee: Option<serde_json::Value>,
    #[serde(default)]
    assignee_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentData {
    #[allow(dead_code)]
    id: String,
    body: String,
    issue: Option<CommentIssue>,
}

#[derive(Debug, Deserialize)]
struct CommentIssue {
    identifier: String,
    title: String,
}

#[derive(Debug, Deserialize)]
struct Actor {
    name: String,
    #[allow(dead_code)]
    email: Option<String>,
}

// ---------------------------------------------------------------------------
// Lark card models
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct LarkMessage {
    msg_type: &'static str,
    card: LarkCard,
}

#[derive(Serialize, Clone)]
struct LarkCard {
    header: LarkHeader,
    elements: Vec<serde_json::Value>,
}

#[derive(Serialize, Clone)]
struct LarkHeader {
    template: String,
    title: LarkTitle,
}

#[derive(Serialize, Clone)]
struct LarkTitle {
    content: String,
    tag: &'static str,
}

// ---------------------------------------------------------------------------
// Signature verification
// ---------------------------------------------------------------------------

fn verify_signature(secret: &str, body: &[u8], signature: &str) -> bool {
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    let expected = hex::encode(mac.finalize().into_bytes());
    expected == signature
}

// ---------------------------------------------------------------------------
// Priority helpers
// ---------------------------------------------------------------------------

fn priority_color(priority: u8) -> &'static str {
    match priority {
        1 => "red",
        2 => "orange",
        3 => "yellow",
        _ => "blue", // 0 (No priority) and 4 (Low)
    }
}

fn priority_label(priority: u8) -> &'static str {
    match priority {
        1 => "Urgent",
        2 => "High",
        3 => "Medium",
        4 => "Low",
        _ => "None",
    }
}

fn priority_emoji(priority: u8) -> &'static str {
    match priority {
        1 => "🔴",
        2 => "🟠",
        3 => "🟡",
        4 => "🔵",
        _ => "⚪",
    }
}

fn priority_display(priority: u8) -> String {
    format!("{} {}", priority_emoji(priority), priority_label(priority))
}

// ---------------------------------------------------------------------------
// Change detection for Issue updates
// ---------------------------------------------------------------------------

fn build_change_fields(issue: &Issue, updated_from: &Option<serde_json::Value>) -> Vec<String> {
    let mut changes = Vec::new();

    let Some(uf_value) = updated_from else {
        return changes;
    };

    let Ok(uf) = serde_json::from_value::<UpdatedFrom>(uf_value.clone()) else {
        return changes;
    };

    // Status change
    if let Some(old_state) = &uf.state {
        let old_name = old_state
            .get("name")
            .and_then(|v| v.as_str())
            // Linear sometimes sends state as a flat string
            .or_else(|| old_state.as_str())
            .unwrap_or("Unknown");
        changes.push(format!("**Status:** {} → {}", old_name, issue.state.name));
    }

    // Priority change
    if let Some(old_priority) = uf.priority {
        changes.push(format!(
            "**Priority:** {} → {}",
            priority_display(old_priority),
            priority_display(issue.priority)
        ));
    }

    // Assignee change
    if uf.assignee_id.is_some() || uf.assignee.is_some() {
        let old_name = uf
            .assignee
            .as_ref()
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unassigned");
        let new_name = issue
            .assignee
            .as_ref()
            .map(|a| a.name.as_str())
            .unwrap_or("Unassigned");
        changes.push(format!("**Assignee:** {} → {}", old_name, new_name));
    }

    changes
}

// ---------------------------------------------------------------------------
// Truncate text helper
// ---------------------------------------------------------------------------

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}…")
    }
}

// ---------------------------------------------------------------------------
// Card event enum & card builder
// ---------------------------------------------------------------------------

enum CardEvent<'a> {
    IssueCreated {
        issue: &'a Issue,
        url: &'a str,
    },
    IssueUpdated {
        issue: &'a Issue,
        url: &'a str,
        changes: Vec<String>,
    },
    CommentCreated {
        comment: &'a CommentData,
        url: &'a str,
        actor: Option<&'a Actor>,
    },
}

fn build_lark_card(event: &CardEvent) -> LarkMessage {
    match event {
        CardEvent::IssueCreated { issue, url } => build_issue_created_card(issue, url),
        CardEvent::IssueUpdated {
            issue,
            url,
            changes,
        } => build_issue_updated_card(issue, url, changes),
        CardEvent::CommentCreated {
            comment,
            url,
            actor,
        } => build_comment_created_card(comment, url, *actor),
    }
}

fn build_issue_created_card(issue: &Issue, url: &str) -> LarkMessage {
    let color = priority_color(issue.priority);

    let mut elements = vec![];

    // Title
    elements.push(serde_json::json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**{}**", issue.title),
        }
    }));

    // Description summary (truncated ~200 chars)
    if let Some(desc) = &issue.description {
        let trimmed = desc.trim();
        if !trimmed.is_empty() {
            elements.push(serde_json::json!({
                "tag": "div",
                "text": {
                    "tag": "lark_md",
                    "content": truncate(trimmed, 200),
                }
            }));
        }
    }

    let assignee = issue
        .assignee
        .as_ref()
        .map(|a| a.name.as_str())
        .unwrap_or("Unassigned");

    // Fields
    elements.push(serde_json::json!({
        "tag": "div",
        "fields": [
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Status:** {}", issue.state.name),
                }
            },
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Priority:** {}", priority_display(issue.priority)),
                }
            },
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Assignee:** {}", assignee),
                }
            }
        ]
    }));

    // Action button
    elements.push(serde_json::json!({
        "tag": "action",
        "actions": [{
            "tag": "button",
            "text": { "tag": "plain_text", "content": "View in Linear" },
            "type": "primary",
            "url": url,
        }]
    }));

    LarkMessage {
        msg_type: "interactive",
        card: LarkCard {
            header: LarkHeader {
                template: color.to_string(),
                title: LarkTitle {
                    content: format!("[Linear] Created: {}", issue.identifier),
                    tag: "plain_text",
                },
            },
            elements,
        },
    }
}

fn build_issue_updated_card(issue: &Issue, url: &str, changes: &[String]) -> LarkMessage {
    let color = priority_color(issue.priority);

    let mut elements = vec![];

    // Title
    elements.push(serde_json::json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**{}**", issue.title),
        }
    }));

    // Change lines
    if !changes.is_empty() {
        let change_text = changes.join("\n");
        elements.push(serde_json::json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": change_text,
            }
        }));
    }

    let assignee = issue
        .assignee
        .as_ref()
        .map(|a| a.name.as_str())
        .unwrap_or("Unassigned");

    // Fields
    elements.push(serde_json::json!({
        "tag": "div",
        "fields": [
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Status:** {}", issue.state.name),
                }
            },
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Priority:** {}", priority_display(issue.priority)),
                }
            },
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Assignee:** {}", assignee),
                }
            }
        ]
    }));

    // Action button
    elements.push(serde_json::json!({
        "tag": "action",
        "actions": [{
            "tag": "button",
            "text": { "tag": "plain_text", "content": "View in Linear" },
            "type": "primary",
            "url": url,
        }]
    }));

    LarkMessage {
        msg_type: "interactive",
        card: LarkCard {
            header: LarkHeader {
                template: color.to_string(),
                title: LarkTitle {
                    content: format!("[Linear] Updated: {}", issue.identifier),
                    tag: "plain_text",
                },
            },
            elements,
        },
    }
}

fn build_comment_created_card(
    comment: &CommentData,
    url: &str,
    actor: Option<&Actor>,
) -> LarkMessage {
    let commenter = actor.map(|a| a.name.as_str()).unwrap_or("Someone");
    let issue_ref = comment
        .issue
        .as_ref()
        .map(|i| format!("{}: {}", i.identifier, i.title))
        .unwrap_or_else(|| "an issue".to_string());

    let mut elements = vec![];

    // Who commented on what
    elements.push(serde_json::json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**{}** commented on **{}**", commenter, issue_ref),
        }
    }));

    // Truncated comment body
    let body = truncate(comment.body.trim(), 200);
    if !body.is_empty() {
        elements.push(serde_json::json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": body,
            }
        }));
    }

    // Action button
    elements.push(serde_json::json!({
        "tag": "action",
        "actions": [{
            "tag": "button",
            "text": { "tag": "plain_text", "content": "View in Linear" },
            "type": "primary",
            "url": url,
        }]
    }));

    let identifier = comment
        .issue
        .as_ref()
        .map(|i| i.identifier.as_str())
        .unwrap_or("?");

    LarkMessage {
        msg_type: "interactive",
        card: LarkCard {
            header: LarkHeader {
                template: "blue".to_string(),
                title: LarkTitle {
                    content: format!("[Linear] Comment: {}", identifier),
                    tag: "plain_text",
                },
            },
            elements,
        },
    }
}

// ---------------------------------------------------------------------------
// Build DM card for assignee notification (Phase 2)
// ---------------------------------------------------------------------------

fn build_assign_dm_card(issue: &Issue, url: &str) -> LarkCard {
    let mut elements = vec![];

    elements.push(serde_json::json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!(
                "You've been assigned to **{}**\n{}",
                issue.identifier, issue.title
            ),
        }
    }));

    elements.push(serde_json::json!({
        "tag": "div",
        "fields": [
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Status:** {}", issue.state.name),
                }
            },
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Priority:** {}", priority_display(issue.priority)),
                }
            }
        ]
    }));

    elements.push(serde_json::json!({
        "tag": "action",
        "actions": [{
            "tag": "button",
            "text": { "tag": "plain_text", "content": "View in Linear" },
            "type": "primary",
            "url": url,
        }]
    }));

    LarkCard {
        header: LarkHeader {
            template: priority_color(issue.priority).to_string(),
            title: LarkTitle {
                content: format!("[Linear] Assigned: {}", issue.identifier),
                tag: "plain_text",
            },
        },
        elements,
    }
}

// ---------------------------------------------------------------------------
// Phase 2: Lark Bot API client (DM via app)
// ---------------------------------------------------------------------------

struct LarkBotClient {
    app_id: String,
    app_secret: String,
    token: tokio::sync::Mutex<CachedToken>,
    http: Client,
}

struct CachedToken {
    value: String,
    expires_at: std::time::Instant,
}

impl LarkBotClient {
    fn new(app_id: String, app_secret: String, http: Client) -> Self {
        Self {
            app_id,
            app_secret,
            token: tokio::sync::Mutex::new(CachedToken {
                value: String::new(),
                expires_at: std::time::Instant::now(),
            }),
            http,
        }
    }

    async fn get_token(&self) -> Result<String, String> {
        let mut cached = self.token.lock().await;

        // Refresh 5 minutes before expiry
        if !cached.value.is_empty()
            && cached.expires_at > std::time::Instant::now() + std::time::Duration::from_secs(300)
        {
            return Ok(cached.value.clone());
        }

        let resp = self
            .http
            .post("https://open.larksuite.com/open-apis/auth/v3/tenant_access_token/internal")
            .json(&serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .send()
            .await
            .map_err(|e| format!("token request failed: {e}"))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("token response parse failed: {e}"))?;

        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!("token API error: {body}"));
        }

        let token = body
            .get("tenant_access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing tenant_access_token in response".to_string())?
            .to_string();

        let expire = body.get("expire").and_then(|v| v.as_u64()).unwrap_or(7200);

        cached.value = token.clone();
        cached.expires_at = std::time::Instant::now() + std::time::Duration::from_secs(expire);

        info!("refreshed lark bot tenant access token (expires in {expire}s)");
        Ok(token)
    }

    async fn send_dm(&self, email: &str, card: &LarkCard) -> Result<(), String> {
        let token = self.get_token().await?;

        let payload = serde_json::json!({
            "receive_id": email,
            "msg_type": "interactive",
            "content": serde_json::to_string(card).unwrap_or_default(),
        });

        let resp = self
            .http
            .post("https://open.larksuite.com/open-apis/im/v1/messages?receive_id_type=email")
            .header("Authorization", format!("Bearer {token}"))
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("DM request failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if status.is_success() {
            let parsed: serde_json::Value =
                serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            let code = parsed.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
            if code != 0 {
                return Err(format!("DM API returned code {code}: {body}"));
            }
            info!("DM sent to {email}");
            Ok(())
        } else {
            Err(format!("DM request returned {status}: {body}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 3: Linear GraphQL client
// ---------------------------------------------------------------------------

struct LinearClient {
    api_key: String,
    http: Client,
}

#[derive(Debug, Deserialize)]
struct LinearIssueData {
    title: String,
    description: Option<String>,
    priority: u8,
    state: LinearIssueState,
    assignee: Option<LinearIssueAssignee>,
    url: String,
    identifier: String,
}

#[derive(Debug, Deserialize)]
struct LinearIssueState {
    name: String,
}

#[derive(Debug, Deserialize)]
struct LinearIssueAssignee {
    name: String,
}

impl LinearClient {
    fn new(api_key: String, http: Client) -> Self {
        Self { api_key, http }
    }

    async fn fetch_issue_by_identifier(
        &self,
        identifier: &str,
    ) -> Result<LinearIssueData, String> {
        let query = r#"
            query IssueByIdentifier($id: String!) {
                issue(id: $id) {
                    title
                    description
                    priority
                    identifier
                    url
                    state { name }
                    assignee { name }
                }
            }
        "#;

        let resp = self
            .http
            .post("https://api.linear.app/graphql")
            .header("Authorization", &self.api_key)
            .json(&serde_json::json!({
                "query": query,
                "variables": { "id": identifier }
            }))
            .send()
            .await
            .map_err(|e| format!("Linear API request failed: {e}"))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Linear API response parse failed: {e}"))?;

        if let Some(errors) = body.get("errors") {
            return Err(format!("Linear GraphQL errors: {errors}"));
        }

        let issue_value = body
            .get("data")
            .and_then(|d| d.get("issue"))
            .ok_or_else(|| "missing data.issue in Linear response".to_string())?;

        serde_json::from_value(issue_value.clone())
            .map_err(|e| format!("failed to deserialize Linear issue: {e}"))
    }
}

/// Extract issue identifier from a Linear URL like
/// `https://linear.app/workspace/issue/LIN-123/some-slug`
fn extract_identifier_from_url(url: &str) -> Option<String> {
    // Match /issue/IDENT pattern
    let parts: Vec<&str> = url.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if *part == "issue" {
            if let Some(ident) = parts.get(i + 1) {
                // Identifier looks like "TEAM-123"
                if ident.contains('-')
                    && ident
                        .split('-')
                        .last()
                        .map(|n| n.chars().all(|c| c.is_ascii_digit()))
                        .unwrap_or(false)
                {
                    return Some(ident.to_string());
                }
            }
        }
    }
    None
}

fn build_preview_card(issue: &LinearIssueData) -> LarkCard {
    let color = priority_color(issue.priority);
    let assignee = issue
        .assignee
        .as_ref()
        .map(|a| a.name.as_str())
        .unwrap_or("Unassigned");

    let mut elements = vec![];

    elements.push(serde_json::json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**{}**", issue.title),
        }
    }));

    if let Some(desc) = &issue.description {
        let trimmed = desc.trim();
        if !trimmed.is_empty() {
            elements.push(serde_json::json!({
                "tag": "div",
                "text": {
                    "tag": "lark_md",
                    "content": truncate(trimmed, 200),
                }
            }));
        }
    }

    elements.push(serde_json::json!({
        "tag": "div",
        "fields": [
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Status:** {}", issue.state.name),
                }
            },
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Priority:** {}", priority_display(issue.priority)),
                }
            },
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Assignee:** {}", assignee),
                }
            }
        ]
    }));

    elements.push(serde_json::json!({
        "tag": "action",
        "actions": [{
            "tag": "button",
            "text": { "tag": "plain_text", "content": "View in Linear" },
            "type": "primary",
            "url": issue.url,
        }]
    }));

    LarkCard {
        header: LarkHeader {
            template: color.to_string(),
            title: LarkTitle {
                content: format!("[Linear] {}", issue.identifier),
                tag: "plain_text",
            },
        },
        elements,
    }
}

// ---------------------------------------------------------------------------
// Webhook handler
// ---------------------------------------------------------------------------

async fn webhook_handler(
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

async fn lark_event_handler(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> (StatusCode, axum::Json<serde_json::Value>) {
    let body_value: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            error!("failed to parse lark event body: {e}");
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({"error": "invalid json"})),
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
            axum::Json(serde_json::json!({ "challenge": challenge })),
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
                axum::Json(serde_json::json!({"error": "invalid token"})),
            );
        }
    }

    // Handle URL preview event
    let event_type = body_value
        .get("header")
        .and_then(|h| h.get("event_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if event_type == "im.message.link_preview.pull" {
        return handle_link_preview(&state, &body_value).await;
    }

    info!("ignoring lark event type: {event_type}");
    (StatusCode::OK, axum::Json(serde_json::json!({})))
}

async fn handle_link_preview(
    state: &AppState,
    body: &serde_json::Value,
) -> (StatusCode, axum::Json<serde_json::Value>) {
    let Some(ref linear) = state.linear_client else {
        warn!("link preview requested but LINEAR_API_KEY not configured");
        return (StatusCode::OK, axum::Json(serde_json::json!({})));
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
        return (StatusCode::OK, axum::Json(serde_json::json!({})));
    };

    info!("fetching Linear issue {identifier} for link preview");

    match linear.fetch_issue_by_identifier(&identifier).await {
        Ok(issue) => {
            let card = build_preview_card(&issue);
            // Return the card in Lark's expected format
            (
                StatusCode::OK,
                axum::Json(serde_json::json!({
                    "preview": {
                        "template": {
                            "type": "interactive",
                            "data": card
                        }
                    }
                })),
            )
        }
        Err(e) => {
            error!("failed to fetch Linear issue {identifier}: {e}");
            (StatusCode::OK, axum::Json(serde_json::json!({})))
        }
    }
}

// ---------------------------------------------------------------------------
// Health-check
// ---------------------------------------------------------------------------

async fn health() -> &'static str {
    "ok"
}

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let webhook_secret =
        env::var("LINEAR_WEBHOOK_SECRET").expect("LINEAR_WEBHOOK_SECRET must be set");
    let lark_webhook_url = env::var("LARK_WEBHOOK_URL").unwrap_or_else(|_| {
        warn!("LARK_WEBHOOK_URL not set – lark notifications will fail");
        String::new()
    });
    let port = env::var("PORT").unwrap_or_else(|_| "3000".into());

    // Phase 2: Optional Lark Bot client for DMs
    let lark_bot = match (env::var("LARK_APP_ID"), env::var("LARK_APP_SECRET")) {
        (Ok(app_id), Ok(app_secret)) => {
            info!("lark bot configured – DM notifications enabled");
            Some(LarkBotClient::new(app_id, app_secret, Client::new()))
        }
        _ => {
            info!("LARK_APP_ID/LARK_APP_SECRET not set – DM notifications disabled");
            None
        }
    };

    // Phase 3: Optional Linear API client for link previews
    let linear_client = env::var("LINEAR_API_KEY").ok().map(|api_key| {
        info!("LINEAR_API_KEY set – link preview enabled");
        LinearClient::new(api_key, Client::new())
    });

    let lark_verification_token = env::var("LARK_VERIFICATION_TOKEN").ok();
    if lark_verification_token.is_some() {
        info!("LARK_VERIFICATION_TOKEN set – event verification enabled");
    }

    let state = Arc::new(AppState {
        webhook_secret,
        lark_webhook_url,
        http: Client::new(),
        lark_bot,
        linear_client,
        lark_verification_token,
    });

    let app = Router::new()
        .route("/webhook", post(webhook_handler))
        .route("/lark/event", post(lark_event_handler))
        .route("/health", get(health))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    info!("listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, app).await.expect("server error");
}
