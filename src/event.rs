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

/// Abbreviated commit info for push events.
#[derive(Serialize, Deserialize, Clone)]
pub struct CommitSummary {
    pub sha_short: String,
    pub message_line: String,
    pub author: String,
}

/// A normalized event produced by a source and consumed by sinks.
#[derive(Serialize, Deserialize)]
pub enum Event {
    // --- Linear events ---
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

    // --- GitHub events ---
    PrOpened {
        repo: String,
        number: u64,
        title: String,
        author: String,
        head_branch: String,
        base_branch: String,
        additions: u64,
        deletions: u64,
        url: String,
    },
    PrReviewRequested {
        repo: String,
        number: u64,
        title: String,
        author: String,
        reviewer: String,
        reviewer_lark_id: Option<String>,
        url: String,
    },
    PrMerged {
        repo: String,
        number: u64,
        title: String,
        author: String,
        merged_by: String,
        url: String,
    },
    IssueLabeledAlert {
        repo: String,
        number: u64,
        title: String,
        label: String,
        author: String,
        url: String,
    },
    BranchPush {
        repo: String,
        branch: String,
        pusher: String,
        commits: Vec<CommitSummary>,
        compare_url: String,
    },
    WorkflowRunFailed {
        repo: String,
        workflow_name: String,
        branch: String,
        actor: String,
        conclusion: String,
        url: String,
    },
    SecretScanningAlert {
        repo: String,
        secret_type: String,
        url: String,
    },
    DependabotAlert {
        repo: String,
        package: String,
        severity: String,
        summary: String,
        url: String,
    },
}

impl Event {
    /// Returns the accumulated change descriptions (empty for non-issue events).
    pub fn changes(&self) -> &[String] {
        match self {
            Event::IssueCreated { changes, .. } | Event::IssueUpdated { changes, .. } => changes,
            _ => &[],
        }
    }

    /// Replaces the change descriptions (no-op for non-issue events).
    pub fn set_changes(&mut self, new_changes: Vec<String>) {
        match self {
            Event::IssueCreated { changes, .. } | Event::IssueUpdated { changes, .. } => {
                *changes = new_changes;
            }
            _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    // -- Priority mapping -----------------------------------------------------

    #[test]
    fn priority_from_linear_known_values() {
        assert!(matches!(Priority::from_linear(1), Priority::Urgent));
        assert!(matches!(Priority::from_linear(2), Priority::High));
        assert!(matches!(Priority::from_linear(3), Priority::Medium));
        assert!(matches!(Priority::from_linear(4), Priority::Low));
    }

    #[test]
    fn priority_from_linear_unknown_defaults_to_none() {
        assert!(matches!(Priority::from_linear(0), Priority::None));
        assert!(matches!(Priority::from_linear(5), Priority::None));
        assert!(matches!(Priority::from_linear(255), Priority::None));
    }

    #[test]
    fn priority_display_format() {
        assert_eq!(Priority::Urgent.display(), "🔴 Urgent");
        assert_eq!(Priority::None.display(), "⚪ None");
    }

    // -- Event helpers --------------------------------------------------------

    fn make_issue_updated(changes: Vec<String>) -> Event {
        Event::IssueUpdated {
            source: "linear".into(),
            identifier: "ENG-1".into(),
            title: "test".into(),
            description: None,
            status: "In Progress".into(),
            priority: Priority::High,
            assignee: None,
            assignee_email: None,
            url: "https://example.com".into(),
            changes,
        }
    }

    fn make_issue_created(changes: Vec<String>) -> Event {
        Event::IssueCreated {
            source: "linear".into(),
            identifier: "ENG-1".into(),
            title: "test".into(),
            description: None,
            status: "In Progress".into(),
            priority: Priority::High,
            assignee: None,
            assignee_email: None,
            url: "https://example.com".into(),
            changes,
        }
    }

    #[test]
    fn changes_returns_vec_for_issue_events() {
        let ev = make_issue_updated(vec!["status changed".into()]);
        assert_eq!(ev.changes(), &["status changed".to_string()]);
    }

    #[test]
    fn changes_returns_empty_for_non_issue_events() {
        let ev = Event::PrMerged {
            repo: "r".into(),
            number: 1,
            title: "t".into(),
            author: "a".into(),
            merged_by: "m".into(),
            url: "u".into(),
        };
        assert!(ev.changes().is_empty());
    }

    #[test]
    fn promote_to_created_converts_updated() {
        let ev = make_issue_updated(vec!["change".into()]);
        let promoted = ev.promote_to_created();
        assert!(promoted.is_issue_created());
        assert_eq!(promoted.changes(), &["change".to_string()]);
    }

    #[test]
    fn promote_to_created_preserves_created() {
        let ev = make_issue_created(vec![]);
        let promoted = ev.promote_to_created();
        assert!(promoted.is_issue_created());
    }

    #[test]
    fn promote_to_created_no_op_for_other_variants() {
        let ev = Event::PrMerged {
            repo: "r".into(),
            number: 1,
            title: "t".into(),
            author: "a".into(),
            merged_by: "m".into(),
            url: "u".into(),
        };
        let result = ev.promote_to_created();
        assert!(!result.is_issue_created());
    }

    #[test]
    fn set_changes_updates_issue_event() {
        let mut ev = make_issue_updated(vec!["old".into()]);
        ev.set_changes(vec!["new1".into(), "new2".into()]);
        assert_eq!(ev.changes(), &["new1".to_string(), "new2".to_string()]);
    }
}
