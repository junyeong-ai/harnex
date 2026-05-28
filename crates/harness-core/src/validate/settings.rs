//! Validator for `.claude/settings.json`.
//!
//! Checks:
//! - JSON parses.
//! - Every key under `hooks` is a documented event name per
//!   <https://code.claude.com/docs/en/hooks>. The known set is a
//!   permissive superset whose job is catching typo'd event names —
//!   not asserting an exact spec count (the surface evolves upstream).
//! - `permissions.deny` is present and non-empty (warn-only — small projects
//!   may legitimately have no denies, but the absence is worth surfacing).
//! - `permissions.defaultMode` is in the closed enum
//!   `KNOWN_DEFAULT_MODE_VALUES` if present.
//! - Settings keys that silently no-op outside user/managed scope
//!   (`KNOWN_PROJECT_SCOPE_NOOP_KEYS`) appearing in a project / local
//!   `settings.json` — per the live `/en/settings` doc, these look
//!   effective but become no-ops.
//! - `skillOverrides` values are valid trigger modes.
//! - Overly permissive `permissions.allow` patterns without a corresponding deny.

use std::path::Path;

use serde_json::Value;

use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};

/// Valid values for `skillOverrides` per Claude Code spec.
pub const KNOWN_SKILL_OVERRIDE_VALUES: &[&str] = &["on", "name-only", "user-invocable-only", "off"];

/// Closed enum of `permissions.defaultMode` values per /en/settings.
/// `auto` is technically a valid wire value but silently no-ops outside
/// user/managed scope — see [`KNOWN_PROJECT_SCOPE_NOOP_KEYS`] handling.
pub const KNOWN_DEFAULT_MODE_VALUES: &[&str] = &[
    "default",
    "acceptEdits",
    "plan",
    "auto",
    "dontAsk",
    "bypassPermissions",
];

/// Keys that the live /en/settings doc documents as silently ignored in
/// project / local `settings.json`. Per Claude Code, they are honored only
/// in user / managed scopes. Emitting them into a project/local file looks
/// effective but does nothing — a generated harness must never contain them.
///
/// The `defaultMode: "auto"` entry is special: the key itself is valid, only
/// the `auto` value no-ops at project/local scope. See `validate_text` for the
/// value-aware branch.
pub const KNOWN_PROJECT_SCOPE_NOOP_KEYS: &[&str] = &[
    "autoMemoryDirectory",
    "autoMode",
    "useAutoModeDuringPlan",
    "skipDangerousModePermissionPrompt",
    "claudeMd",
];

/// Closed-set of `settings.json` scopes per Claude Code spec /en/settings.
///
/// Scope decides which keys / values are honored: certain settings
/// (`defaultMode: "auto"`, `autoMemoryDirectory`, `autoMode`,
/// `useAutoModeDuringPlan`, `skipDangerousModePermissionPrompt`) silently
/// no-op outside user / managed scope, so the validator must know its scope
/// to fire the right
/// findings. Caller-provided rather than path-inferred — path heuristics
/// (HOME env, filename) are platform-brittle and the caller already knows
/// which file it loaded.
///
/// Four variants rather than a binary (`ProjectLocalOrNot`) because:
/// 1. Operator UX — the `--scope` CLI flag displays the full set; a binary
///    would lose the labeling.
/// 2. Future scope-specific checks — managed-only keys
///    (`allowManagedPermissionRulesOnly`, `strictPluginOnlyCustomization`,
///    …) should eventually fire only at `Managed` scope. The variant is
///    ready for that check without a shape change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsScope {
    /// `<project>/.claude/settings.json` — committed, team-shared.
    Project,
    /// `<project>/.claude/settings.local.json` — gitignored, per-developer.
    Local,
    /// `~/.claude/settings.json` — per-user, all projects.
    User,
    /// Org-managed (`/Library/Application Support/ClaudeCode/managed-settings.json`,
    /// `/etc/claude-code/managed-settings.json`, Windows registry / plist).
    Managed,
}

impl SettingsScope {
    pub const ALL: &'static [Self] = &[Self::Project, Self::Local, Self::User, Self::Managed];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "project" => Self::Project,
            "local" => Self::Local,
            "user" => Self::User,
            "managed" => Self::Managed,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Local => "local",
            Self::User => "user",
            Self::Managed => "managed",
        }
    }

    /// True for the scopes where keys in [`KNOWN_PROJECT_SCOPE_NOOP_KEYS`]
    /// (and the `defaultMode: "auto"` value) silently no-op. User and
    /// managed scope honor those settings; project and local do not.
    pub fn project_scope_noop_applies(self) -> bool {
        matches!(self, Self::Project | Self::Local)
    }
}

/// Command bases that are overly permissive when broadly allowed without a
/// corresponding deny. Compared against the normalized base of each rule, so
/// detection is independent of the wildcard spelling (`Bash(rm:*)`,
/// `Bash(rm *)`, and `Bash(rm)` all normalize to `rm`). A *scoped* rule like
/// `Bash(curl https://api *)` normalizes to a longer base and is not flagged.
const DANGEROUS_ALLOW_BASES: &[&str] = &["rm", "rm -rf", "curl", "sudo"];

/// Reduce a `Bash(...)` rule to its command base by stripping the wrapper and
/// the equivalent trailing wildcard forms (`:*`, ` *`, bare). Non-Bash rules
/// return `None`. Per the Claude Code spec, `Bash(cmd:*)` ≡ `Bash(cmd *)`, so
/// both must collapse to the same base for style-independent matching.
fn bash_command_base(rule: &str) -> Option<String> {
    let inner = rule.strip_prefix("Bash(")?.strip_suffix(')')?;
    Some(
        inner
            .trim_end_matches('*')
            .trim_end_matches(':')
            .trim()
            .to_string(),
    )
}

/// Documented hook event names per Claude Code spec /en/hooks.
/// A permissive superset for typo detection — membership errs toward
/// accepting, so a newly-added upstream event is never falsely flagged;
/// the check exists only to catch misspelled event keys that silently
/// no-op. Source-of-truth for SettingsValidator and skill `hooks` keys.
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
    "MessageDisplay",
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

    pub fn validate_file(&self, path: &Path, scope: SettingsScope) -> Result<Vec<Finding>> {
        let contents = std::fs::read_to_string(path).map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(self.validate_text(&contents, path, scope))
    }

    pub fn validate_text(&self, content: &str, path: &Path, scope: SettingsScope) -> Vec<Finding> {
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

        // No-deny advisory: fires whether `permissions` is absent entirely
        // (no guardrails at all — the riskiest case) or present with an
        // empty/missing deny array.
        let perms = parsed.get("permissions").and_then(|v| v.as_object());
        let deny_empty = perms
            .and_then(|p| p.get("deny"))
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
                fix_command: Some("harness policy permissions generate --profile baseline".into()),
            });
        }

        // Overly permissive allow patterns are only meaningful when a
        // permissions block exists.
        if let Some(perms) = perms {
            let allow_strs: Vec<&str> = perms
                .get("allow")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();
            let deny_strs: Vec<&str> = perms
                .get("deny")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();
            for allow in &allow_strs {
                let Some(base) = bash_command_base(allow) else {
                    continue;
                };
                if !DANGEROUS_ALLOW_BASES.contains(&base.as_str()) {
                    continue;
                }
                // Excused only by a deny of the same command base — matched
                // independent of wildcard spelling, so a `Bash(rm *)` allow is
                // covered by a `Bash(rm:*)` deny and vice versa.
                let covered = deny_strs
                    .iter()
                    .any(|d| bash_command_base(d).as_deref() == Some(base.as_str()));
                if !covered {
                    findings.push(Finding {
                        slug: "settings-overly-permissive".into(),
                        severity: Severity::Minor,
                        location: Location::file(path.to_path_buf()),
                        message: format!(
                            "'{allow}' in permissions.allow without a corresponding deny"
                        ),
                        hint: Some("move this pattern to deny or scope it more tightly".into()),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        // permissions.defaultMode: closed-enum value check
        if let Some(mode) = parsed.pointer("/permissions/defaultMode") {
            match mode.as_str() {
                Some(s) if KNOWN_DEFAULT_MODE_VALUES.contains(&s) => {
                    if s == "auto" && scope.project_scope_noop_applies() {
                        findings.push(Finding {
                            slug: "settings-project-scope-noop-value".into(),
                            severity: Severity::Major,
                            location: Location::file(path.to_path_buf()),
                            message:
                                "permissions.defaultMode = \"auto\" is silently ignored in project/local settings (honored only in user/managed scope)"
                                    .into(),
                            hint: Some(
                                "remove the key or move it to ~/.claude/settings.json".into(),
                            ),
                            auto_fixable: false,
                            fix_command: None,
                        });
                    }
                }
                Some(s) => {
                    findings.push(Finding {
                        slug: "settings-default-mode-invalid".into(),
                        severity: Severity::Major,
                        location: Location::file(path.to_path_buf()),
                        message: format!(
                            "permissions.defaultMode '{s}' is not a valid mode; must be one of: {}",
                            KNOWN_DEFAULT_MODE_VALUES.join(", ")
                        ),
                        hint: Some(
                            "set defaultMode to default, acceptEdits, plan, auto, dontAsk, or bypassPermissions".into(),
                        ),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
                None => {
                    findings.push(Finding {
                        slug: "settings-default-mode-invalid".into(),
                        severity: Severity::Major,
                        location: Location::file(path.to_path_buf()),
                        message: "permissions.defaultMode must be a string".into(),
                        hint: Some(
                            "set defaultMode to default, acceptEdits, plan, auto, dontAsk, or bypassPermissions".into(),
                        ),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        // Keys silently ignored at project / local scope. A generated harness
        // that emits them is a configuration bug because they look effective
        // but no-op. Scope is caller-provided; user / managed scopes honor
        // these keys and never reach this branch.
        if scope.project_scope_noop_applies() {
            for key in KNOWN_PROJECT_SCOPE_NOOP_KEYS {
                if parsed.get(*key).is_some() {
                    findings.push(Finding {
                        slug: "settings-project-scope-noop-key".into(),
                        severity: Severity::Major,
                        location: Location::file(path.to_path_buf()),
                        message: format!(
                            "'{key}' is silently ignored in project/local settings (honored only in user/managed scope)"
                        ),
                        hint: Some(format!(
                            "remove '{key}' or move it to ~/.claude/settings.json"
                        )),
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
                    if !KNOWN_SKILL_OVERRIDE_VALUES.contains(&s) {
                        findings.push(Finding {
                            slug: "settings-skill-override-invalid".into(),
                            severity: Severity::Major,
                            location: Location::file(path.to_path_buf()),
                            message: format!(
                                "skillOverrides['{skill_name}'] value '{s}' is not valid; \
                                 must be one of: {}",
                                KNOWN_SKILL_OVERRIDE_VALUES.join(", ")
                            ),
                            hint: Some("set to on, name-only, user-invocable-only, or off".into()),
                            auto_fixable: false,
                            fix_command: None,
                        });
                    }
                } else {
                    findings.push(Finding {
                        slug: "settings-skill-override-invalid".into(),
                        severity: Severity::Major,
                        location: Location::file(path.to_path_buf()),
                        message: format!("skillOverrides['{skill_name}'] must be a string"),
                        hint: Some("set to on, name-only, user-invocable-only, or off".into()),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::SettingsScope;

    #[test]
    fn scope_from_str_round_trips_every_variant() {
        for scope in SettingsScope::ALL {
            assert_eq!(SettingsScope::from_str(scope.as_str()), Some(*scope));
        }
    }

    #[test]
    fn scope_from_str_rejects_unknown() {
        assert!(SettingsScope::from_str("unknown-scope").is_none());
    }

    #[test]
    fn project_scope_noop_applies_on_project_and_local() {
        assert!(SettingsScope::Project.project_scope_noop_applies());
        assert!(SettingsScope::Local.project_scope_noop_applies());
        assert!(!SettingsScope::User.project_scope_noop_applies());
        assert!(!SettingsScope::Managed.project_scope_noop_applies());
    }
}
