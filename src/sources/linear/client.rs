//! Linear GraphQL API client for fetching issue data (used by link previews).

use std::collections::HashMap;

use reqwest::Client;
use serde_json::json;
use tokio::sync::RwLock;
use tracing::warn;

use super::models::LinearIssueData;

/// Client for the Linear GraphQL API.
pub struct LinearClient {
    api_key: String,
    http: Client,
    /// Lazily populated `workflow-state id → name` cache.
    state_cache: RwLock<HashMap<String, String>>,
}

impl LinearClient {
    pub fn new(api_key: String, http: Client) -> Self {
        Self {
            api_key,
            http,
            state_cache: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the display name for a workflow state UUID.
    ///
    /// On a cache miss, fetches all workflow states from Linear and populates
    /// the cache before returning. Returns `None` only if the API call fails.
    pub async fn state_name(&self, id: &str) -> Option<String> {
        // Fast path: read lock only.
        {
            let cache = self.state_cache.read().await;
            if let Some(name) = cache.get(id) {
                return Some(name.clone());
            }
        }

        // Cache miss — fetch all workflow states and repopulate.
        match self.fetch_workflow_states().await {
            Ok(map) => {
                let name = map.get(id).cloned();
                *self.state_cache.write().await = map;
                name
            }
            Err(e) => {
                warn!("failed to fetch workflow states: {e}");
                None
            }
        }
    }

    /// Fetches a single issue by its identifier (e.g. `"ABX-16"`).
    pub async fn fetch_issue_by_identifier(
        &self,
        identifier: &str,
    ) -> Result<LinearIssueData, String> {
        let (team_key, number_str) = identifier
            .rsplit_once('-')
            .ok_or_else(|| format!("invalid identifier format: {identifier}"))?;
        let number: u32 = number_str
            .parse()
            .map_err(|_| format!("invalid issue number in identifier: {identifier}"))?;

        let query = r#"
            query IssueByNumber($teamKey: String!, $number: Float!) {
                issues(filter: {
                    number: { eq: $number },
                    team: { key: { eq: $teamKey } }
                }, first: 1) {
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
                "variables": {
                    "teamKey": team_key,
                    "number": number
                }
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
            .ok_or_else(|| {
                format!("no issue found for identifier '{identifier}' – body: {body}")
            })?;

        serde_json::from_value(issue_value.clone())
            .map_err(|e| format!("failed to deserialize Linear issue: {e}"))
    }

    async fn fetch_workflow_states(&self) -> Result<HashMap<String, String>, String> {
        let query = "{ workflowStates { nodes { id name } } }";

        let resp = self
            .http
            .post("https://api.linear.app/graphql")
            .header("Authorization", &self.api_key)
            .json(&json!({ "query": query }))
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("parse failed: {e}"))?;

        let nodes = body["data"]["workflowStates"]["nodes"]
            .as_array()
            .ok_or_else(|| format!("unexpected response shape: {body}"))?;

        Ok(nodes
            .iter()
            .filter_map(|n| {
                let id = n["id"].as_str()?.to_string();
                let name = n["name"].as_str()?.to_string();
                Some((id, name))
            })
            .collect())
    }
}

/// Extracts an issue identifier (e.g. `"LIN-123"`) from a Linear URL like
/// `https://linear.app/workspace/issue/LIN-123/some-slug`.
pub fn extract_identifier_from_url(url: &str) -> Option<String> {
    let parts: Vec<&str> = url.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if *part == "issue"
            && let Some(ident) = parts.get(i + 1)
            && ident.contains('-')
            && ident
                .split('-')
                .next_back()
                .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
        {
            return Some(ident.to_string());
        }
    }
    None
}
