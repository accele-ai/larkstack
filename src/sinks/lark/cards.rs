//! Converts [`Event`]s and [`LinearIssueData`] into Lark interactive cards.

use serde_json::{json, Value};

use crate::{
    event::{Event, Priority},
    sources::linear::models::LinearIssueData,
    utils::truncate,
};

use super::models::{LarkCard, LarkHeader, LarkMessage, LarkTitle};

/// Returns the Lark header color template for a given priority.
fn priority_color(priority: &Priority) -> &'static str {
    match priority {
        Priority::Urgent => "red",
        Priority::High => "orange",
        Priority::Medium => "yellow",
        _ => "blue",
    }
}

/// Builds a "Status / Priority / Assignee" fields block.
fn build_fields(status: &str, priority: &str, assignee: Option<&str>) -> Value {
    let assignee = assignee.unwrap_or("Unassigned");
    let fields = vec![
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
    json!({ "tag": "div", "fields": fields })
}

/// Builds a "View in Linear" action button element.
fn build_action_button(url: &str) -> Value {
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

/// Formats an [`Event`] as a [`LarkMessage`] for group webhook delivery.
pub fn build_lark_card(event: &Event) -> LarkMessage {
    match event {
        Event::IssueCreated {
            identifier,
            title,
            description,
            status,
            priority,
            assignee,
            url,
            changes,
            ..
        } => build_issue_card(
            "Created",
            identifier,
            title,
            description.as_deref(),
            status,
            priority,
            assignee.as_deref(),
            url,
            changes,
        ),
        Event::IssueUpdated {
            identifier,
            title,
            status,
            priority,
            assignee,
            url,
            changes,
            ..
        } => build_issue_card(
            "Updated",
            identifier,
            title,
            None,
            status,
            priority,
            assignee.as_deref(),
            url,
            changes,
        ),
        Event::CommentCreated {
            identifier,
            issue_title,
            author,
            body,
            url,
            ..
        } => build_comment_card(identifier, issue_title, author, body, url),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_issue_card(
    action: &str,
    identifier: &str,
    title: &str,
    description: Option<&str>,
    status: &str,
    priority: &Priority,
    assignee: Option<&str>,
    url: &str,
    changes: &[String],
) -> LarkMessage {
    let color = priority_color(priority);
    let assignee_name = assignee.unwrap_or("Unassigned");

    let mut elements = vec![];

    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**{title}**"),
        }
    }));

    if let Some(desc) = description {
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

    elements.push(build_fields(status, &priority.display(), Some(assignee_name)));
    elements.push(build_action_button(url));

    LarkMessage {
        msg_type: "interactive",
        card: LarkCard {
            header: LarkHeader {
                template: color.to_string(),
                title: LarkTitle {
                    content: format!("[Linear] {action}: {identifier}"),
                    tag: "plain_text",
                },
            },
            elements,
        },
    }
}

fn build_comment_card(
    identifier: &str,
    issue_title: &str,
    author: &str,
    body: &str,
    url: &str,
) -> LarkMessage {
    let issue_ref = if issue_title.is_empty() {
        "an issue".to_string()
    } else {
        format!("{identifier}: {issue_title}")
    };

    let mut elements = vec![];

    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**{author}** commented on **{issue_ref}**"),
        }
    }));

    let body = truncate(body.trim(), 200);
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

    LarkMessage {
        msg_type: "interactive",
        card: LarkCard {
            header: LarkHeader {
                template: "blue".to_string(),
                title: LarkTitle {
                    content: format!("[Linear] Comment: {identifier}"),
                    tag: "plain_text",
                },
            },
            elements,
        },
    }
}

/// Builds a DM card notifying the assignee about an issue event.
///
/// # Panics
///
/// Panics if called with [`Event::CommentCreated`].
pub fn build_assign_dm_card(event: &Event) -> LarkCard {
    let (identifier, title, status, priority, url) = match event {
        Event::IssueCreated {
            identifier,
            title,
            status,
            priority,
            url,
            ..
        }
        | Event::IssueUpdated {
            identifier,
            title,
            status,
            priority,
            url,
            ..
        } => (
            identifier.as_str(),
            title.as_str(),
            status.as_str(),
            priority,
            url.as_str(),
        ),
        Event::CommentCreated { .. } => unreachable!("build_assign_dm_card called with comment"),
    };

    let mut elements = vec![];

    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!(
                "You've been assigned to **{}**\n{}",
                identifier, title
            ),
        }
    }));

    elements.push(build_fields(status, &priority.display(), None));
    elements.push(build_action_button(url));

    LarkCard {
        header: LarkHeader {
            template: priority_color(priority).to_string(),
            title: LarkTitle {
                content: format!("[Linear] Assigned: {identifier}"),
                tag: "plain_text",
            },
        },
        elements,
    }
}

/// Builds an inline preview card from GraphQL-fetched issue data.
///
/// This is used for Lark link unfurling and does **not** go through [`Event`].
pub fn build_preview_card(issue: &LinearIssueData) -> LarkCard {
    let priority = Priority::from_linear(issue.priority);
    let color = priority_color(&priority);
    let assignee = issue
        .assignee
        .as_ref()
        .map(|a| a.name.as_str())
        .unwrap_or("Unassigned");

    let mut elements = vec![];

    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**{}**", issue.title),
        }
    }));

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
        &priority.display(),
        Some(assignee),
    ));
    elements.push(build_action_button(&issue.url));

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
