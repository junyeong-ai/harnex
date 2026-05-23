//! Validator for `.claude/settings.json`.
//!
//! Checks:
//! - JSON parses.
//! - Every key under `hooks` is one of the 29 documented event names
//!   per <https://code.claude.com/docs/en/hooks>.
//! - `permissions.deny` is present and non-empty (warn-only — small projects
//!   may legitimately have no denies, but the absence is worth surfacing).
//! - `skillOverrides` values are valid trigger modes.
//! - Overly permissive `permissions.allow` patterns without a corresponding deny.
//! - `autoMemoryEnabled` presence noted as intentional configuration.

use std::path::Path;

use serde_json::Value;

use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};

/// Valid values for `skillOverrides` per Claude Code spec.
const VALID_SKILL_OVERRIDE_VALUES: &[&str] = &["on", "name-only", "user-invocable-only", "off"];

/// Patterns in `permissions.allow` that are overly permissive without a
/// corresponding deny entry.
const DANGEROUS_ALLOW_PATTERNS: &[&str] = &[
    "Bash(rm:*)",
    "Bash(curl:*)",
    "Bash(sudo:*)",
    "Bash(rm -rf:*)",
];

/// All hook event names per Claude Code spec /en/hooks (29 total).
/// Source-of-truth. Used by SettingsValidator and exposed for tests.
pub const KNOWN_HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "Setup",
    "UserPromptSubmit",
    "UserPromptExpansion",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PostToolBatch",
    "PermissionRequest",
    "PermissionDenied",
    "Stop",
    "StopFailure",
    "SubagentStart",
    "SubagentStop",
    "Notification",
    "PreCompact",
    "PostCompact",
    "InstructionsLoaded",
    "ConfigChange",
    "CwdChanged",
    "FileChanged",
    "WorktreeCreate",
    "WorktreeRemove",
    "TaskCreated",
    "TaskCompleted",
    "TeammateIdle",
    "Elicitation",
    "ElicitationResult",
];

#[derive(Default)]
pub struct SettingsValidator;

impl SettingsValidator {
    pub fn new() -> Self {
        Self
    }

    pub fn validate_file(&self, path: &Path) -> Result<Vec<Finding>> {
        let contents = std::fs::read_to_string(path).map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(self.validate_text(&contents, path))
    }

    pub fn validate_text(&self, content: &str, path: &Path) -> Vec<Finding> {
        let mut findings = Vec::new();
        let parsed: Value = match serde_json::from_str(content) {
            Ok(v) => v,
            Err(e) => {
                findings.push(Finding {
                    slug: "settings-json-invalid".into(),
                    severity: Severity::Blocker,
                    location: Location::line(path.to_path_buf(), e.line() as u32),
                    message: format!("json parse: {e}"),
                    hint: Some(
                        "fix the JSON syntax; if the file is empty or corrupted, regenerate via \
                         `harness policy permissions generate --profile baseline > .claude/settings.json`"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
                return findings;
            }
        };

        if let Some(hooks) = parsed.get("hooks").and_then(|v| v.as_object()) {
            for event_name in hooks.keys() {
                if !KNOWN_HOOK_EVENTS.contains(&event_name.as_str()) {
                    findings.push(Finding {
                        slug: "settings-unknown-hook-event".into(),
                        severity: Severity::Major,
                        location: Location::file(path.to_path_buf()),
                        message: format!(
                            "hook event '{event_name}' is not in the Claude Code spec /en/hooks"
                        ),
                        hint: Some(format!("known events: {}", KNOWN_HOOK_EVENTS.join(", "))),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        if let Some(perms) = parsed.get("permissions").and_then(|v| v.as_object()) {
            let deny_empty = perms
                .get("deny")
                .and_then(|v| v.as_array())
                .map(|a| a.is_empty())
                .unwrap_or(true);
            if deny_empty {
                findings.push(Finding {
                    slug: "settings-no-deny-rules".into(),
                    severity: Severity::Minor,
                    location: Location::file(path.to_path_buf()),
                    message: "permissions.deny is missing or empty".into(),
                    hint: Some("seed it via `harness policy permissions generate`".into()),
                    auto_fixable: false,
                    fix_command: Some(
                        "harness policy permissions generate --profile baseline".into(),
                    ),
                });
            }

            // Overly permissive allow patterns without corresponding deny
            let allow_strs: Vec<&str> = perms
                .get("allow")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let deny_strs: Vec<&str> = perms
                .get("deny")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for pattern in DANGEROUS_ALLOW_PATTERNS {
                if allow_strs.iter().any(|a| a == pattern)
                    && !deny_strs.iter().any(|d| d == pattern)
                {
                    findings.push(Finding {
                        slug: "settings-overly-permissive".into(),
                        severity: Severity::Minor,
                        location: Location::file(path.to_path_buf()),
                        message: format!(
                            "'{pattern}' in permissions.allow without a corresponding deny"
                        ),
                        hint: Some(
                            "move this pattern to deny or scope it more tightly".into(),
                        ),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        // skillOverrides: each value must be a valid trigger mode
        if let Some(overrides) = parsed.get("skillOverrides").and_then(|v| v.as_object()) {
            for (skill_name, mode) in overrides {
                if let Some(s) = mode.as_str() {
                    if !VALID_SKILL_OVERRIDE_VALUES.contains(&s) {
                        findings.push(Finding {
                            slug: "settings-skill-override-invalid".into(),
                            severity: Severity::Major,
                            location: Location::file(path.to_path_buf()),
                            message: format!(
                                "skillOverrides['{skill_name}'] value '{s}' is not valid; \
                                 must be one of: {}",
                                VALID_SKILL_OVERRIDE_VALUES.join(", ")
                            ),
                            hint: Some(
                                "set to on, name-only, user-invocable-only, or off".into(),
                            ),
                            auto_fixable: false,
                            fix_command: None,
                        });
                    }
                } else {
                    findings.push(Finding {
                        slug: "settings-skill-override-invalid".into(),
                        severity: Severity::Major,
                        location: Location::file(path.to_path_buf()),
                        message: format!(
                            "skillOverrides['{skill_name}'] must be a string"
                        ),
                        hint: Some(
                            "set to on, name-only, user-invocable-only, or off".into(),
                        ),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        // autoMemoryEnabled: informational acknowledgement
        if parsed.get("autoMemoryEnabled").is_some() {
            findings.push(Finding {
                slug: "settings-auto-memory-configured".into(),
                severity: Severity::Info,
                location: Location::file(path.to_path_buf()),
                message: "autoMemoryEnabled is explicitly configured".into(),
                hint: Some(
                    "this is an intentional team decision; no action needed".into(),
                ),
                auto_fixable: false,
                fix_command: None,
            });
        }

        findings
    }
}
