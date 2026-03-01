//! Serializable Lark interactive card structures.

use serde::Serialize;

/// A complete Lark card message ready to POST.
#[derive(Serialize)]
pub struct LarkMessage {
    pub msg_type: &'static str,
    pub card: LarkCard,
}

/// The card body (header + elements).
#[derive(Serialize, Clone)]
pub struct LarkCard {
    pub header: LarkHeader,
    pub elements: Vec<serde_json::Value>,
}

/// Card header with a color template and title.
#[derive(Serialize, Clone)]
pub struct LarkHeader {
    pub template: String,
    pub title: LarkTitle,
}

/// Plain-text title used inside a [`LarkHeader`].
#[derive(Serialize, Clone)]
pub struct LarkTitle {
    pub content: String,
    pub tag: &'static str,
}
