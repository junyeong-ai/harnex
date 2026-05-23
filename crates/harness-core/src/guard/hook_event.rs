//! Typed parsing of Claude Code hook stdin JSON.
//!
//! The toolkit does not model event-specific fields as Rust types — the
//! 29-event surface is too wide and evolves upstream. Common fields are
//! extracted; the rest stays accessible via [`HookEvent::field`].

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct HookEvent {
    pub hook_event_name: String,
    pub session_id: String,
    pub cwd: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// Raw event JSON. Event-specific fields are accessed via [`field`].
    #[serde(flatten)]
    pub raw: serde_json::Map<String, serde_json::Value>,
}

impl HookEvent {
    pub fn from_stdin_json(raw: &str) -> Result<Self> {
        serde_json::from_str(raw).map_err(|e| Error::GuardHookInputInvalid {
            message: format!("hook stdin json: {e}"),
        })
    }

    /// Reach into event-specific raw fields by key.
    pub fn field(&self, key: &str) -> Option<&serde_json::Value> {
        self.raw.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stop_event() {
        let raw = r#"{
            "hook_event_name": "Stop",
            "session_id": "abc123",
            "cwd": "/tmp/project",
            "transcript_path": "/tmp/t.jsonl",
            "permission_mode": "default",
            "effort": {"level": "high"}
        }"#;
        let ev = HookEvent::from_stdin_json(raw).unwrap();
        assert_eq!(ev.hook_event_name, "Stop");
        assert_eq!(ev.session_id, "abc123");
        assert!(ev.field("effort").is_some());
    }

    #[test]
    fn rejects_malformed_input() {
        let raw = r#"{not json"#;
        let err = HookEvent::from_stdin_json(raw).unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::GuardHookInputInvalid);
    }
}
