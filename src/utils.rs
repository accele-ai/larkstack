use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::models::{Issue, UpdatedFrom};

// ---------------------------------------------------------------------------
// Signature verification
// ---------------------------------------------------------------------------

pub fn verify_signature(secret: &str, body: &[u8], signature: &str) -> bool {
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

pub fn priority_color(priority: u8) -> &'static str {
    match priority {
        1 => "red",
        2 => "orange",
        3 => "yellow",
        _ => "blue", // 0 (No priority) and 4 (Low)
    }
}

pub fn priority_label(priority: u8) -> &'static str {
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

pub fn priority_display(priority: u8) -> String {
    format!("{} {}", priority_emoji(priority), priority_label(priority))
}

// ---------------------------------------------------------------------------
// Change detection for Issue updates
// ---------------------------------------------------------------------------

pub fn build_change_fields(issue: &Issue, updated_from: &Option<serde_json::Value>) -> Vec<String> {
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

pub fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}…")
    }
}
