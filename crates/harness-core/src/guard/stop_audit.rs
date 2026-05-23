//! Fresh-context Stop auditor.
//!
//! Flow:
//! 1. Check `has_changes_check` — if exit 0 (no changes), allow stop.
//! 2. Bump per-session retry counter; if > max_retries, escalate via Block.
//! 3. Spawn the critique skill via `claude --print <critique_skill>` from
//!    the working directory.
//! 4. Parse critique output as JSON; if any finding has severity in
//!    {blocker}, return Block; otherwise reset counter and Allow.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::config::StopAuditConfig;
use crate::error::{Error, Result};
use crate::path_guard;

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(tag = "decision", rename_all = "kebab-case")]
pub enum StopDecision {
    Allow,
    Block { reason: String },
}

pub struct StopAuditor<'a> {
    config: &'a StopAuditConfig,
    working_dir: &'a Path,
    session_id: String,
}

impl<'a> StopAuditor<'a> {
    pub fn new(config: &'a StopAuditConfig, working_dir: &'a Path, session_id: String) -> Self {
        Self {
            config,
            working_dir,
            session_id: safe_session_id(&session_id),
        }
    }

    pub fn run(&self) -> Result<StopDecision> {
        if !self.has_changes()? {
            return Ok(StopDecision::Allow);
        }
        let attempt = self.bump_retry_counter()?;
        if attempt > self.config.max_retries {
            return Ok(StopDecision::Block {
                reason: format!(
                    "retry counter exceeded {} — escalating to user",
                    self.config.max_retries
                ),
            });
        }
        let critique_output = self.spawn_critique()?;
        if has_blocker(&critique_output) {
            Ok(StopDecision::Block {
                reason: format!(
                    "critique skill '{}' returned blocker-severity findings",
                    self.config.critique_skill
                ),
            })
        } else {
            self.clear_retry_counter()?;
            Ok(StopDecision::Allow)
        }
    }

    fn has_changes(&self) -> Result<bool> {
        if self.config.has_changes_check.is_empty() {
            return Ok(true);
        }
        let (program, args) = self.config.has_changes_check.split_first().unwrap();
        let status = Command::new(program)
            .args(args)
            .current_dir(self.working_dir)
            .status()
            .map_err(|e| Error::GuardSpawnFailure {
                message: format!("has_changes_check spawn: {e}"),
            })?;
        // Convention: exit 0 == no changes; non-zero == changes present.
        Ok(!status.success())
    }

    fn retry_path(&self) -> PathBuf {
        self.working_dir
            .join(&self.config.retry_ledger_dir)
            .join(format!("{}.count", self.session_id))
    }

    fn bump_retry_counter(&self) -> Result<u32> {
        let path = self.retry_path();
        let current = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        let next = current + 1;
        path_guard::write_atomic(&path, next.to_string().as_bytes())?;
        Ok(next)
    }

    fn clear_retry_counter(&self) -> Result<()> {
        let path = self.retry_path();
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| Error::IoFailure {
                path: path.clone(),
                source: e,
            })?;
        }
        Ok(())
    }

    fn spawn_critique(&self) -> Result<String> {
        let output = Command::new("claude")
            .args(["--print", &self.config.critique_skill])
            .current_dir(self.working_dir)
            .output()
            .map_err(|e| Error::GuardSpawnFailure {
                message: format!("spawn 'claude': {e}"),
            })?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

fn safe_session_id(raw: &str) -> String {
    if !raw.is_empty()
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        raw.to_string()
    } else {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        raw.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

/// Inspect a JSON envelope payload for any finding with severity in
/// {blocker}. Returns false on parse failure (fail-open).
fn has_blocker(critique_output: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(critique_output) else {
        return false;
    };
    let Some(items) = value
        .get("data")
        .and_then(|d| d.get("items"))
        .and_then(|i| i.as_array())
    else {
        return false;
    };
    items.iter().any(|item| {
        item.get("severity")
            .and_then(|s| s.as_str())
            .is_some_and(|s| s == "blocker")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_blocker_detects_blocker_finding() {
        let json = r#"{"ok":true,"data":{"items":[
            {"slug":"x","severity":"blocker","location":{"path":"a"},"message":"oops"}
        ],"total":1}}"#;
        assert!(has_blocker(json));
    }

    #[test]
    fn has_blocker_ignores_minor_findings() {
        let json = r#"{"ok":true,"data":{"items":[
            {"slug":"x","severity":"minor","location":{"path":"a"},"message":"meh"}
        ],"total":1}}"#;
        assert!(!has_blocker(json));
    }

    #[test]
    fn has_blocker_handles_empty_findings() {
        let json = r#"{"ok":true,"data":{"items":[],"total":0}}"#;
        assert!(!has_blocker(json));
    }

    #[test]
    fn has_blocker_handles_parse_failure() {
        assert!(!has_blocker("not json"));
    }

    #[test]
    fn safe_session_id_passes_valid_ids() {
        assert_eq!(safe_session_id("abc-123_XYZ"), "abc-123_XYZ");
        assert_eq!(safe_session_id("simple"), "simple");
    }

    #[test]
    fn safe_session_id_sanitizes_path_separators() {
        let sanitized = safe_session_id("../../etc/passwd");
        assert!(
            sanitized.chars().all(|c| c.is_ascii_hexdigit()),
            "expected hex hash, got: {sanitized}"
        );
        assert_eq!(sanitized.len(), 16);
    }

    #[test]
    fn safe_session_id_sanitizes_empty() {
        let sanitized = safe_session_id("");
        assert!(
            sanitized.chars().all(|c| c.is_ascii_hexdigit()),
            "expected hex hash, got: {sanitized}"
        );
        assert_eq!(sanitized.len(), 16);
    }
}
