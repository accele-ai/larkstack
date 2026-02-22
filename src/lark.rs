use reqwest::Client;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::info;

use crate::{
    models::{Actor, CommentData, Issue, LarkCard, LarkHeader, LarkMessage, LarkTitle},
    utils::{priority_color, priority_display, truncate},
};

// ---------------------------------------------------------------------------
// Card event enum & card builder
// ---------------------------------------------------------------------------

pub enum CardEvent<'a> {
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

pub fn build_lark_card(event: &CardEvent) -> LarkMessage {
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
    elements.push(json!({
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
            elements.push(json!({
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
    elements.push(json!({
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
    elements.push(json!({
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
    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**{}**", issue.title),
        }
    }));

    // Change lines
    if !changes.is_empty() {
        let change_text = changes.join("
");
        elements.push(json!({
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
    elements.push(json!({
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
    elements.push(json!({
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
    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**{}** commented on **{}**", commenter, issue_ref),
        }
    }));

    // Truncated comment body
    let body = truncate(comment.body.trim(), 200);
    if !body.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": body,
            }
        }));
    }

    // Action button
    elements.push(json!({
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

pub fn build_assign_dm_card(issue: &Issue, url: &str) -> LarkCard {
    let mut elements = vec![];

    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!(
                "You've been assigned to **{}**
{}",
                issue.identifier, issue.title
            ),
        }
    }));

    elements.push(json!({
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

    elements.push(json!({
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

pub struct LarkBotClient {
    app_id: String,
    app_secret: String,
    token: Mutex<CachedToken>,
    http: Client,
}

struct CachedToken {
    value: String,
    expires_at: std::time::Instant,
}

impl LarkBotClient {
    pub fn new(app_id: String, app_secret: String, http: Client) -> Self {
        Self {
            app_id,
            app_secret,
            token: Mutex::new(CachedToken {
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
            .json(&json!({
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

    pub async fn send_dm(&self, email: &str, card: &LarkCard) -> Result<(), String> {
        let token = self.get_token().await?;

        let payload = json!({
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
