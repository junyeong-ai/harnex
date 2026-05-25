//! Spec-drift auditor for `.claude/settings.json`.
//!
//! Catches values that look plausible but violate the live Claude Code
//! spec. Every check is deterministic — no prose pattern matching, no
//! NLP-style heuristics with false-positive floors. Each finding is
//! either provably-wrong against the spec or provably-redundant.
//!
//! Checks:
//! - Hook `timeout` ≥ 1000 → almost certainly milliseconds by mistake
//!   (the spec uses seconds; 1000s exceeds every documented default
//!   ceiling of 600s and an upper bound of 60 minutes is generous).
//! - `mcp__<server>` matcher without `__.*` → matches nothing per spec.
//! - `Stop` event hook without an apparent exit-0 contract (non-`_stop_`
//!   wrapper script names) → likely Stop-loop hazard.
//!
//! ## What this module refuses to do
//!
//! - Never read shell-script bodies to assess control flow — that path
//!   is unbounded heuristic territory. The Stop check is a NAME-only
//!   probe: if the script name doesn't carry the `_stop_` convention,
//!   surface it as `Info` for review. Stronger detection lives in the
//!   `_stop_runner.sh` template's `exit 0` contract.

use std::path::Path;

use serde_json::Value;

use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};

/// Above this value, a hook `timeout` is almost certainly milliseconds by
/// mistake. The Claude Code spec uses seconds with documented defaults
/// 600/30/60 — a four-figure number reaches an hour, well past every
/// documented use case. Per `keep-soften-cut`, numeric caps are advisory:
/// the finding is severity `Minor` because a legitimately long timeout
/// (e.g., a slow scaffolded build hook) is plausible.
const TIMEOUT_MS_SUSPICION_THRESHOLD: u64 = 1000;

/// Naming convention the harnex Stop-class wrappers carry. The name-based
/// probe is a heuristic — a Python or non-`_stop_` wrapper that correctly
/// returns exit 0 would not match. The strong contract lives in
/// `harness_core::guard::HookRunner::run_stop`. The finding is severity
/// `Info` so the heuristic never blocks; the type-level contract on the
/// Rust runner is the enforcement layer.
const STOP_RUNNER_TOKEN: &str = "_stop_runner";

#[derive(Default)]
pub(crate) struct SettingsDriftAuditor;

impl SettingsDriftAuditor {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn audit_file(&self, path: &Path) -> Result<Vec<Finding>> {
        let raw = std::fs::read_to_string(path).map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        let value: Value = serde_json::from_str(&raw).map_err(|e| Error::ConfigInvalid {
            message: format!("settings.json parse: {e}"),
            location: Some(Location::file(path.to_path_buf())),
        })?;
        Ok(self.audit_value(&value, path))
    }

    pub(crate) fn audit_value(&self, value: &Value, path: &Path) -> Vec<Finding> {
        let mut findings = Vec::new();
        let Some(hooks) = value.get("hooks").and_then(|v| v.as_object()) else {
            return findings;
        };
        for (event_name, event_arr) in hooks {
            let Some(entries) = event_arr.as_array() else {
                continue;
            };
            for (entry_idx, entry) in entries.iter().enumerate() {
                self.audit_matcher(entry, event_name, entry_idx, path, &mut findings);
                self.audit_handlers(entry, event_name, entry_idx, path, &mut findings);
            }
        }
        findings
    }

    fn audit_matcher(
        &self,
        entry: &Value,
        event_name: &str,
        entry_idx: usize,
        path: &Path,
        findings: &mut Vec<Finding>,
    ) {
        let Some(matcher) = entry.get("matcher").and_then(|v| v.as_str()) else {
            return;
        };
        // Only a BARE `mcp__<server>` matches nothing — an exact-string
        // matcher (per the spec, `[A-Za-z0-9_|]` only) with no `__<tool>`
        // segment. A matcher carrying a regex metacharacter (`mcp__.*`,
        // `mcp__.*__write.*`) is a JS regex that DOES match — never flag it.
        if let Some(after) = matcher.strip_prefix("mcp__") {
            let is_regex = after
                .chars()
                .any(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '|'));
            let has_tool_segment = after.contains("__");
            if !is_regex && !has_tool_segment {
                findings.push(Finding {
                    slug: "audit-mcp-matcher-incomplete".into(),
                    severity: Severity::Major,
                    location: Location::file(path.to_path_buf()),
                    message: format!(
                        "hook '{event_name}'[{entry_idx}] matcher '{matcher}' matches no MCP tool — \
                         bare 'mcp__{after}' is an exact string with no tool segment. Use \
                         'mcp__{after}__<tool>' for one tool, 'mcp__{after}__.*' for all of that \
                         server, or 'mcp__.*' for every MCP tool"
                    ),
                    hint: Some(
                        "add the `__<tool>` segment or a regex wildcard; bare `mcp__server` is a no-op"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }
    }

    fn audit_handlers(
        &self,
        entry: &Value,
        event_name: &str,
        entry_idx: usize,
        path: &Path,
        findings: &mut Vec<Finding>,
    ) {
        let Some(handlers) = entry.get("hooks").and_then(|v| v.as_array()) else {
            return;
        };
        for (handler_idx, handler) in handlers.iter().enumerate() {
            if let Some(t) = handler.get("timeout").and_then(|v| v.as_u64())
                && t >= TIMEOUT_MS_SUSPICION_THRESHOLD
            {
                findings.push(Finding {
                    slug: "audit-ms-timeout".into(),
                    severity: Severity::Minor,
                    location: Location::file(path.to_path_buf()),
                    message: format!(
                        "hook '{event_name}'[{entry_idx}].hooks[{handler_idx}] timeout = {t} — \
                         Claude Code timeouts are in SECONDS (defaults 600/30/60); {t} looks like milliseconds"
                    ),
                    hint: Some("rewrite as seconds (e.g., 30 instead of 30000)".into()),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
            if event_name == "Stop" || event_name == "SubagentStop" || event_name == "StopFailure" {
                let command = handler
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !command.contains(STOP_RUNNER_TOKEN) {
                    findings.push(Finding {
                        slug: "audit-stop-blocking-suspect".into(),
                        severity: Severity::Info,
                        location: Location::file(path.to_path_buf()),
                        message: format!(
                            "Stop-class hook '{event_name}'[{entry_idx}].hooks[{handler_idx}] command does not route through a `_stop_runner` wrapper — \
                             a non-zero exit here triggers the Claude Code re-stop loop"
                        ),
                        hint: Some(
                            "wrap the inner script in `hooks/_stop_runner.sh <script>` so non-zero \
                             observations are reported but never propagated"
                                .into(),
                        ),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn run_on(json: &str) -> Vec<Finding> {
        let value: Value = serde_json::from_str(json).expect("test json parses");
        SettingsDriftAuditor::new().audit_value(&value, &PathBuf::from(".claude/settings.json"))
    }

    #[test]
    fn flags_millisecond_timeout() {
        let json = r#"{
            "hooks": {
                "PostToolUse": [{
                    "matcher": "Edit",
                    "hooks": [{"type": "command", "command": "hooks/_runner.sh format.sh", "timeout": 30000}]
                }]
            }
        }"#;
        let findings = run_on(json);
        assert!(
            findings.iter().any(|f| f.slug == "audit-ms-timeout"
                && matches!(f.severity, Severity::Minor)
                && f.message.contains("30000")),
            "expected audit-ms-timeout (Minor — keep-soften-cut): {findings:?}"
        );
    }

    #[test]
    fn accepts_seconds_timeout() {
        let json = r#"{
            "hooks": {
                "PostToolUse": [{
                    "matcher": "Edit",
                    "hooks": [{"type": "command", "command": "hooks/_runner.sh format.sh", "timeout": 15}]
                }]
            }
        }"#;
        let findings = run_on(json);
        assert!(
            !findings.iter().any(|f| f.slug == "audit-ms-timeout"),
            "15-second timeout must not be flagged: {findings:?}"
        );
    }

    #[test]
    fn flags_incomplete_mcp_matcher() {
        let json = r#"{
            "hooks": {
                "PreToolUse": [{
                    "matcher": "mcp__myserver",
                    "hooks": [{"type": "command", "command": "x"}]
                }]
            }
        }"#;
        let findings = run_on(json);
        assert!(
            findings
                .iter()
                .any(|f| f.slug == "audit-mcp-matcher-incomplete"),
            "expected audit-mcp-matcher-incomplete: {findings:?}"
        );
    }

    #[test]
    fn accepts_complete_mcp_matcher() {
        let json = r#"{
            "hooks": {
                "PreToolUse": [{
                    "matcher": "mcp__myserver__.*",
                    "hooks": [{"type": "command", "command": "x"}]
                }]
            }
        }"#;
        let findings = run_on(json);
        assert!(
            !findings
                .iter()
                .any(|f| f.slug == "audit-mcp-matcher-incomplete"),
            "mcp__server__.* is valid: {findings:?}"
        );
    }

    #[test]
    fn accepts_all_mcp_tools_regex_matcher() {
        // `mcp__.*` is a JS regex (contains `.`) that matches every MCP tool
        // across every server — a common, valid telemetry matcher. It must
        // NOT be flagged as incomplete (regression: real projects use it).
        let json = r#"{
            "hooks": {
                "PreToolUse": [{
                    "matcher": "mcp__.*",
                    "hooks": [{"type": "command", "command": "x"}]
                }]
            }
        }"#;
        let findings = run_on(json);
        assert!(
            !findings
                .iter()
                .any(|f| f.slug == "audit-mcp-matcher-incomplete"),
            "mcp__.* (all MCP tools) is a valid regex matcher: {findings:?}"
        );
    }

    #[test]
    fn accepts_cross_server_regex_matcher() {
        // `mcp__.*__write.*` — write tools from any server. Regex, valid.
        let json = r#"{
            "hooks": {
                "PreToolUse": [{
                    "matcher": "mcp__.*__write.*",
                    "hooks": [{"type": "command", "command": "x"}]
                }]
            }
        }"#;
        let findings = run_on(json);
        assert!(
            !findings
                .iter()
                .any(|f| f.slug == "audit-mcp-matcher-incomplete"),
            "cross-server regex matcher is valid: {findings:?}"
        );
    }

    #[test]
    fn accepts_specific_mcp_tool_matcher() {
        let json = r#"{
            "hooks": {
                "PreToolUse": [{
                    "matcher": "mcp__myserver__deploy",
                    "hooks": [{"type": "command", "command": "x"}]
                }]
            }
        }"#;
        let findings = run_on(json);
        assert!(
            !findings
                .iter()
                .any(|f| f.slug == "audit-mcp-matcher-incomplete"),
            "mcp__server__tool is valid: {findings:?}"
        );
    }

    #[test]
    fn flags_stop_hook_without_stop_runner_wrapper() {
        let json = r#"{
            "hooks": {
                "Stop": [{
                    "hooks": [{"type": "command", "command": "hooks/_runner.sh check-on-stop.sh"}]
                }]
            }
        }"#;
        let findings = run_on(json);
        assert!(
            findings
                .iter()
                .any(|f| f.slug == "audit-stop-blocking-suspect"),
            "expected audit-stop-blocking-suspect: {findings:?}"
        );
    }

    #[test]
    fn accepts_stop_hook_via_stop_runner() {
        let json = r#"{
            "hooks": {
                "Stop": [{
                    "hooks": [{"type": "command", "command": "hooks/_stop_runner.sh check-on-stop.sh"}]
                }]
            }
        }"#;
        let findings = run_on(json);
        assert!(
            !findings
                .iter()
                .any(|f| f.slug == "audit-stop-blocking-suspect"),
            "Stop via _stop_runner is correct: {findings:?}"
        );
    }
}
