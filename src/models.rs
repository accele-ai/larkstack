use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Linear webhook models
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct LinearPayload {
    pub action: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub data: serde_json::Value,
    pub url: String,
    #[serde(rename = "updatedFrom")]
    pub updated_from: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct Issue {
    #[allow(dead_code)]
    pub id: String,
    pub title: String,
    pub priority: u8,
    pub state: IssueState,
    pub assignee: Option<Assignee>,
    pub identifier: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct IssueState {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Assignee {
    pub name: String,
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedFrom {
    #[serde(default)]
    pub state: Option<serde_json::Value>,
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub assignee: Option<serde_json::Value>,
    #[serde(default)]
    pub assignee_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentData {
    #[allow(dead_code)]
    pub id: String,
    pub body: String,
    pub issue: Option<CommentIssue>,
}

#[derive(Debug, Deserialize)]
pub struct CommentIssue {
    pub identifier: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct Actor {
    pub name: String,
    #[allow(dead_code)]
    pub email: Option<String>,
}

// ---------------------------------------------------------------------------
// Lark card models
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LarkMessage {
    pub msg_type: &'static str,
    pub card: LarkCard,
}

#[derive(Serialize, Clone)]
pub struct LarkCard {
    pub header: LarkHeader,
    pub elements: Vec<serde_json::Value>,
}

#[derive(Serialize, Clone)]
pub struct LarkHeader {
    pub template: String,
    pub title: LarkTitle,
}

#[derive(Serialize, Clone)]
pub struct LarkTitle {
    pub content: String,
    pub tag: &'static str,
}

// ---------------------------------------------------------------------------
// Linear GraphQL Client Models
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct LinearIssueData {
    pub title: String,
    pub description: Option<String>,
    pub priority: u8,
    pub state: LinearIssueState,
    pub assignee: Option<LinearIssueAssignee>,
    pub url: String,
    pub identifier: String,
}

#[derive(Debug, Deserialize)]
pub struct LinearIssueState {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct LinearIssueAssignee {
    pub name: String,
}
