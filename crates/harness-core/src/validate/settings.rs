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
//! - Permission rules Claude Code accepts and never consults, per
//!   `harness_core::policy::rule`.

use std::path::Path;

use serde_json::Value;

use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::policy::{PermissionRule, RuleDirection, RuleEffect};
use crate::wire_enum::wire_enum;

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

wire_enum! {
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
        Project => "project",
        /// `<project>/.claude/settings.local.json` — gitignored, per-developer.
        Local => "local",
        /// `~/.claude/settings.json` — per-user, all projects.
        User => "user",
        /// Org-managed (`/Library/Application Support/ClaudeCode/managed-settings.json`,
        /// `/etc/claude-code/managed-settings.json`, Windows registry / plist).
        Managed => "managed",
    }
}

impl SettingsScope {
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

/// The string rules under one `permissions` array. A non-string entry is
/// dropped rather than reported: the settings schema is Claude Code's to
/// enforce, and this validator speaks about rules.
fn rule_array<'a>(perms: &'a serde_json::Map<String, Value>, array: &str) -> Vec<&'a str> {
    perms
        .get(array)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default()
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
    "PreModelSwitch",
    "PostModelSwitch",
    "InstructionsLoaded",
    "ConfigChange",
    "CwdChanged",
    "DirectoryAdded",
    "FileChanged",
    "WorktreeCreate",
    "WorktreeRemove",
    "TaskCreated",
    "TaskCompleted",
    "TeammateIdle",
    "Elicitation",
    "ElicitationResult",
];

/// How a session can start, which is what a `SessionStart` matcher selects.
///
/// The set matters because three of these are context-loss boundaries: after
/// `compact`, `clear`, or `fork`, the model holds none of what a SessionStart
/// hook injected the first time. A matcher naming only `startup|resume` is
/// well-formed and silently absent at exactly the moments its context is worth
/// most.
pub const KNOWN_SESSION_START_SOURCES: &[&str] = &["startup", "resume", "clear", "compact", "fork"];

/// The hooks page's own vocabulary, stamped separately from the settings sets
/// because it is read from a different document. It lives beside its constants
/// like every other `SPEC_SETS`, rather than inline in `spec.rs`.
pub const HOOK_SPEC_SETS: &[(&str, &[&str])] = &[
    ("hook-events", KNOWN_HOOK_EVENTS),
    ("session-start-sources", KNOWN_SESSION_START_SOURCES),
];

/// Every closed set this validator reads from the settings page, labelled.
/// The measurement stamp digests exactly this list, so a value moved from one
/// set to another changes the digest rather than hiding in a concatenation.
pub const SPEC_SETS: &[(&str, &[&str])] = &[
    ("project-scope-noop-keys", KNOWN_PROJECT_SCOPE_NOOP_KEYS),
    ("default-mode-values", KNOWN_DEFAULT_MODE_VALUES),
    ("skill-override-values", KNOWN_SKILL_OVERRIDE_VALUES),
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
                        "fix the JSON syntax. To rebuild the permissions block, declare \
                         `[policy.permissions] profiles` in harness.toml, run `harnex policy \
                         permissions generate`, and copy its `data` under the `permissions` key \
                         — the command emits an envelope, so redirecting it into this file \
                         would leave the file invalid a second time"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
                return findings;
            }
        };

        if let Some(hooks) = parsed.get("hooks").and_then(|v| v.as_object()) {
            for (event_name, entries) in hooks {
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
                if event_name == "SessionStart" {
                    findings.extend(self.session_start_sources(entries, path));
                }
            }
        }

        // No-deny advisory: fires whether `permissions` is absent entirely
        // (no guardrails at all — the riskiest case) or present with an
        // empty/missing deny array.
        //
        // Project scope only. A deny floor is a team guarantee and the
        // committed file is the only scope that carries one; permission rules
        // merge across scopes, so an empty deny in `settings.local.json`
        // withholds nothing — the project deny still applies. Local scope is
        // where a developer records an override, which is exactly what the
        // generated `governance.md` sends them there for, and flagging it made
        // the harness scold an operator for following its own instructions.
        // User and managed scope are outside this project's authority.
        let perms = parsed.get("permissions").and_then(|v| v.as_object());
        let deny_empty = perms
            .and_then(|p| p.get("deny"))
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true);
        if deny_empty && scope == SettingsScope::Project {
            findings.push(Finding {
                slug: "settings-no-deny-rules".into(),
                severity: Severity::Minor,
                location: Location::file(path.to_path_buf()),
                message: "permissions.deny is missing or empty".into(),
                hint: Some("seed it via `harnex policy permissions generate`".into()),
                auto_fixable: false,
                fix_command: None,
            });
        }

        // Rule-level checks are only meaningful when a permissions block
        // exists at all.
        if let Some(perms) = perms {
            let allow_strs = rule_array(perms, "allow");
            let ask_strs = rule_array(perms, "ask");
            let deny_strs = rule_array(perms, "deny");

            for (array, rules, direction) in [
                ("allow", &allow_strs, RuleDirection::Allow),
                ("ask", &ask_strs, RuleDirection::Ask),
                ("deny", &deny_strs, RuleDirection::Deny),
            ] {
                for rule in rules {
                    let parsed = PermissionRule::parse(rule);
                    if let RuleEffect::Inert(inert) = parsed.effect() {
                        findings.push(Finding {
                            slug: "settings-inert-permission-rule".into(),
                            severity: Severity::Major,
                            location: Location::file(path.to_path_buf()),
                            message: format!(
                                "'{rule}' in permissions.{array} is never consulted — {}",
                                inert.reason_text()
                            ),
                            hint: Some(inert.hint()),
                            auto_fixable: false,
                            fix_command: None,
                        });
                        continue;
                    }
                    // Advisory, never gating: the rule functions, so an
                    // incumbent settings file keeps passing — the finding
                    // says the operator holds a different rule than the one
                    // they wrote.
                    if let Some(misleading) = parsed.misleading(direction) {
                        findings.push(Finding {
                            slug: "settings-misleading-permission-rule".into(),
                            severity: Severity::Minor,
                            location: Location::file(path.to_path_buf()),
                            message: format!(
                                "'{rule}' in permissions.{array} reaches other than it reads — {}",
                                misleading.reason_text()
                            ),
                            hint: Some(misleading.hint()),
                            auto_fixable: false,
                            fix_command: None,
                        });
                    }
                }
            }
            for allow in &allow_strs {
                let Some(base) = PermissionRule::parse(allow).bash_base() else {
                    continue;
                };
                if !DANGEROUS_ALLOW_BASES.contains(&base.as_str()) {
                    continue;
                }
                // Excused only by a deny of the same command base — matched
                // independent of wildcard spelling, so a `Bash(rm *)` allow is
                // covered by a `Bash(rm:*)` deny and vice versa.
                let covered = deny_strs.iter().any(|d| {
                    PermissionRule::parse(d).bash_base().as_deref() == Some(base.as_str())
                });
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

    /// Hold a `SessionStart` matcher's alternatives to the documented source
    /// set.
    ///
    /// A matcher carrying no regex metacharacter is judged; one that does is
    /// left alone. `.*` or `st.*` matches sources this set cannot enumerate,
    /// and testing membership on it would flag a working configuration.
    ///
    /// Everything else is a literal, whatever characters it holds. A space, a
    /// hyphen, a comma and an underscore have no regex meaning, so
    /// `startup resume` and `startup-resume` are literal strings that equal no
    /// session source under any matching semantics — they fire for nothing,
    /// which is precisely the defect this check exists to name. Trimming the
    /// alternatives, or bailing out on any non-alphanumeric character, erased
    /// exactly those cases while claiming to catch them.
    ///
    /// A dead alternative is otherwise silent: it matches no session and the
    /// hook simply never fires for it, which reads as the hook working because
    /// the other alternatives still do.
    fn session_start_sources(&self, entries: &Value, path: &Path) -> Vec<Finding> {
        /// Characters that give a matcher regex power. `|` is absent: it is the
        /// alternation this check reads, and the spec's own exact-string form
        /// is a `|`-separated list.
        const REGEX_METACHARACTERS: &[char] = &[
            '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '^', '$', '\\',
        ];

        let mut findings = Vec::new();
        let Some(entries) = entries.as_array() else {
            return findings;
        };
        for entry in entries {
            let Some(matcher) = entry.get("matcher").and_then(Value::as_str) else {
                continue;
            };
            // `*`, `""` and an absent matcher all mean every source, so an
            // empty alternative widens the matcher rather than dying. Reading
            // one as a dead source inverted the truth: the entry it flagged
            // fires for everything, and the equivalent spelling — omitting the
            // key — was already skipped two lines up.
            if matcher.contains(REGEX_METACHARACTERS) || matcher.split('|').any(str::is_empty) {
                continue;
            }
            for unknown in matcher
                .split('|')
                .filter(|a| !KNOWN_SESSION_START_SOURCES.contains(a))
            {
                findings.push(Finding {
                    slug: "settings-unknown-session-start-source".into(),
                    severity: Severity::Major,
                    location: Location::file(path.to_path_buf()),
                    message: format!(
                        "SessionStart matcher '{matcher}' names source '{unknown}', which starts no \
                         session — that alternative never fires"
                    ),
                    hint: Some(format!(
                        "known sources: {}",
                        KNOWN_SESSION_START_SOURCES.join(", ")
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
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
