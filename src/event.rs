//! Unified event model — the middle layer between sources and sinks.
//!
//! Every source converts its platform-specific payload into an [`Event`],
//! which sinks consume without knowing the origin.

use serde::{Deserialize, Serialize};

/// Issue priority level, normalized across all sources.
#[derive(Serialize, Deserialize)]
pub enum Priority {
    None,
    Urgent,
    High,
    Medium,
    Low,
}

impl Priority {
    /// Convert a Linear numeric priority (`0`–`4`) to [`Priority`].
    pub fn from_linear(value: u8) -> Self {
        match value {
            1 => Self::Urgent,
            2 => Self::High,
            3 => Self::Medium,
            4 => Self::Low,
            _ => Self::None,
        }
    }

    /// Human-readable label (e.g. `"Urgent"`).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Urgent => "Urgent",
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
            Self::None => "None",
        }
    }

    /// Colored circle emoji for display.
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Urgent => "🔴",
            Self::High => "🟠",
            Self::Medium => "🟡",
            Self::Low => "🔵",
            Self::None => "⚪",
        }
    }

    /// `"{emoji} {label}"` combined string.
    pub fn display(&self) -> String {
        format!("{} {}", self.emoji(), self.label())
    }
}

/// A normalized event produced by a source and consumed by sinks.
#[derive(Serialize, Deserialize)]
pub enum Event {
    IssueCreated {
        #[allow(dead_code)]
        source: String,
        identifier: String,
        title: String,
        description: Option<String>,
        status: String,
        priority: Priority,
        assignee: Option<String>,
        #[allow(dead_code)]
        assignee_email: Option<String>,
        url: String,
        changes: Vec<String>,
    },
    IssueUpdated {
        #[allow(dead_code)]
        source: String,
        identifier: String,
        title: String,
        description: Option<String>,
        status: String,
        priority: Priority,
        assignee: Option<String>,
        #[allow(dead_code)]
        assignee_email: Option<String>,
        url: String,
        changes: Vec<String>,
    },
    CommentCreated {
        #[allow(dead_code)]
        source: String,
        identifier: String,
        issue_title: String,
        author: String,
        body: String,
        url: String,
    },
}

impl Event {
    /// Returns the accumulated change descriptions (empty for comments).
    pub fn changes(&self) -> &[String] {
        match self {
            Event::IssueCreated { changes, .. } | Event::IssueUpdated { changes, .. } => changes,
            Event::CommentCreated { .. } => &[],
        }
    }

    /// Replaces the change descriptions (no-op for comments).
    pub fn set_changes(&mut self, new_changes: Vec<String>) {
        match self {
            Event::IssueCreated { changes, .. } | Event::IssueUpdated { changes, .. } => {
                *changes = new_changes;
            }
            Event::CommentCreated { .. } => {}
        }
    }

    /// Returns `true` if this is an [`Event::IssueCreated`].
    pub fn is_issue_created(&self) -> bool {
        matches!(self, Event::IssueCreated { .. })
    }

    /// Promotes an [`Event::IssueUpdated`] to [`Event::IssueCreated`],
    /// preserving all fields. Other variants are returned unchanged.
    pub fn promote_to_created(self) -> Self {
        match self {
            Event::IssueUpdated {
                source,
                identifier,
                title,
                description,
                status,
                priority,
                assignee,
                assignee_email,
                url,
                changes,
            } => Event::IssueCreated {
                source,
                identifier,
                title,
                description,
                status,
                priority,
                assignee,
                assignee_email,
                url,
                changes,
            },
            other => other,
        }
    }
}
