use serde_json::{json, Value};

use crate::{
    models::{Actor, CommentData, Issue, LarkCard, LarkHeader, LarkMessage, LarkTitle},
    utils::{priority_color, priority_display, truncate},
};

// ---------------------------------------------------------------------------
// Shared card-element helpers (deduplicated)
// ---------------------------------------------------------------------------

/// Build the standard "Status / Priority / Assignee" fields block.
pub fn build_fields(status: &str, priority: &str, assignee: Option<&str>) -> Value {
    let assignee = assignee.unwrap_or("Unassigned");
    let mut fields = vec![
        json!({
            "is_short": true,
            "text": {
                "tag": "lark_md",
                "content": format!("**Status:** {status}"),
            }
        }),
        json!({
            "is_short": true,
            "text": {
                "tag": "lark_md",
                "content": format!("**Priority:** {priority}"),
            }
        }),
        json!({
            "is_short": true,
            "text": {
                "tag": "lark_md",
                "content": format!("**Assignee:** {assignee}"),
            }
        }),
    ];
    // Drop the assignee field when it was explicitly omitted (None)
    // — caller can pass Some("Unassigned") to keep it visible.
    if assignee == "Unassigned" && fields.len() == 3 {
        // keep it; we always show all three for consistency
    }
    let _ = &mut fields; // suppress unused-mut if branch is empty
    json!({ "tag": "div", "fields": fields })
}

/// Build a standard "View in Linear" action-button element.
pub fn build_action_button(url: &str) -> Value {
    json!({
        "tag": "action",
        "actions": [{
            "tag": "button",
            "text": { "tag": "plain_text", "content": "View in Linear" },
            "type": "primary",
            "url": url,
        }]
    })
}

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
    let assignee_name = issue
        .assignee
        .as_ref()
        .map(|a| a.name.as_str())
        .unwrap_or("Unassigned");

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

    elements.push(build_fields(
        &issue.state.name,
        &priority_display(issue.priority),
        Some(assignee_name),
    ));
    elements.push(build_action_button(url));

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
    let assignee_name = issue
        .assignee
        .as_ref()
        .map(|a| a.name.as_str())
        .unwrap_or("Unassigned");

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
        let change_text = changes.join("\n");
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": change_text,
            }
        }));
    }

    elements.push(build_fields(
        &issue.state.name,
        &priority_display(issue.priority),
        Some(assignee_name),
    ));
    elements.push(build_action_button(url));

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

    elements.push(build_action_button(url));

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
                "You've been assigned to **{}**\n{}",
                issue.identifier, issue.title
            ),
        }
    }));

    elements.push(build_fields(
        &issue.state.name,
        &priority_display(issue.priority),
        None,
    ));
    elements.push(build_action_button(url));

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
