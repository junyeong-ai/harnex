//! Loosely-typed node reference. Captures the well-known fields (id, kind,
//! path, status) and flattens everything else into `extra` so an upstream
//! schema addition doesn't require a toolkit release.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NodeRef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// All non-well-known fields from the nodex response.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Loose representation of `nodex diff a b` output. Captures the
/// well-known top-level keys and flattens unknown fields into `extra`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GraphDiff {
    #[serde(default)]
    pub added_nodes: Vec<NodeRef>,
    #[serde(default)]
    pub removed_nodes: Vec<NodeRef>,
    #[serde(default)]
    pub status_transitions: Vec<serde_json::Value>,
    #[serde(default)]
    pub field_changes: Vec<serde_json::Value>,
    #[serde(default)]
    pub added_edges: Vec<serde_json::Value>,
    #[serde(default)]
    pub removed_edges: Vec<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
