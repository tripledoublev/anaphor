use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub(crate) struct ContextBlock {
    pub(crate) label: String,
    pub(crate) text: String,
    pub(crate) url: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SearchResult {
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) snippet: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ToolRequest {
    SearchWeb { query: String },
    ReadUrl { url: String },
    ReadFile { path: String },
}
