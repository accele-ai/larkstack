use reqwest::Client;
use serde_json::json;

use crate::{
    models::{LinearIssueData, LarkCard, LarkHeader, LarkTitle},
    utils::{priority_color, priority_display, truncate},
};

// ---------------------------------------------------------------------------
// Phase 3: Linear GraphQL client
// ---------------------------------------------------------------------------

pub struct LinearClient {
    api_key: String,
    http: Client,
}

impl LinearClient {
    pub fn new(api_key: String, http: Client) -> Self {
        Self { api_key, http }
    }

    pub async fn fetch_issue_by_identifier(
        &self,
        identifier: &str,
    ) -> Result<LinearIssueData, String> {
        let query = r#"
            query IssueByIdentifier($identifier: String!) {
                issues(filter: { identifier: { eq: $identifier } }, first: 1) {
                    nodes {
                        title
                        description
                        priority
                        identifier
                        url
                        state { name }
                        assignee { name }
                    }
                }
            }
        "#;

        let resp = self
            .http
            .post("https://api.linear.app/graphql")
            .header("Authorization", &self.api_key)
            .json(&json!({
                "query": query,
                "variables": { "identifier": identifier }
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
            .and_then(|d| d.get("issues"))
            .and_then(|i| i.get("nodes"))
            .and_then(|n| n.get(0))
            .ok_or_else(|| format!("no issue found for identifier '{identifier}' – body: {body}"))?;

        serde_json::from_value(issue_value.clone())
            .map_err(|e| format!("failed to deserialize Linear issue: {e}"))
    }
}

/// Extract issue identifier from a Linear URL like
/// `https://linear.app/workspace/issue/LIN-123/some-slug`
pub fn extract_identifier_from_url(url: &str) -> Option<String> {
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

pub fn build_preview_card(issue: &LinearIssueData) -> LarkCard {
    let color = priority_color(issue.priority);
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

    elements.push(json!({
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
