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
//! - A key or value the file's scope does not take effect from, per the
//!   Scope column of the settings reference ([`ScopeClass`]): it reads as
//!   configured and changes nothing.
//! - `skillOverrides` values are valid trigger modes.
//! - `claudeMdExcludes` patterns in the committed file that harnex does not
//!   read as the runtime does (`always_loaded`), so what their glob excludes
//!   is still counted and checked here.
//! - Overly permissive `permissions.allow` patterns without a corresponding deny.
//! - Permission rules Claude Code accepts and never consults, per
//!   `harness_core::policy::rule`.

use std::path::Path;

use serde_json::Value;

use crate::envelope::{Finding, Location, Severity};
use crate::policy::{PermissionRule, RuleDirection, RuleEffect};
use crate::wire_enum::wire_enum;

/// `skillOverrides` values, per /en/settings-reference.
pub const KNOWN_SKILL_OVERRIDE_VALUES: &[&str] = &["on", "name-only", "user-invocable-only", "off"];

/// `permissions.defaultMode` values, per /en/settings-reference. `manual` is an
/// alias for `default`.
pub const KNOWN_DEFAULT_MODE_VALUES: &[&str] = &[
    "default",
    "acceptEdits",
    "plan",
    "auto",
    "dontAsk",
    "bypassPermissions",
    "manual",
];

/// Keys the settings reference scopes to managed settings.
pub const MANAGED_SCOPE_KEYS: &[&str] = &[
    "allowAllClaudeAiMcps",
    "allowClaudeInChromeWithManagedMcp",
    "allowManagedHooksOnly",
    "allowManagedMcpServersOnly",
    "allowManagedPermissionRulesOnly",
    "allowedChannelPlugins",
    "allowedProviders",
    "availableModelsMatch",
    "blockedMarketplaces",
    "browserExternalPageTools",
    "channelsEnabled",
    "claudeMd",
    "deniedModels",
    "disableBrowserExternalNavigation",
    "disableCommandPluginSources",
    "disableDesktopLocalSessions",
    "disableMobileSimulatorTools",
    "disableRemotePreviewScripts",
    "disableSideloadFlags",
    "forceRemoteSettingsRefresh",
    "gatewayInternalNetworks",
    "managedMcpServers",
    "managedSourcesBehavior",
    "modelPricing",
    "parentSettingsBehavior",
    "pluginSuggestionMarketplaces",
    "pluginTrustMessage",
    "policyHelper",
    "policyHelper.path",
    "policyHelper.refreshIntervalMs",
    "policyHelper.timeoutMs",
    "requiredMaximumVersion",
    "requiredMinimumVersion",
    "sandbox.bwrapPath",
    "sandbox.filesystem.allowManagedReadPathsOnly",
    "sandbox.network.allowManagedDomainsOnly",
    "sandbox.socatPath",
    "sshHostAllowlist",
    "strictKnownMarketplaces",
    "strictPluginOnlyCustomization",
    "strictPluginOnlyCustomization.agents",
    "strictPluginOnlyCustomization.hooks",
    "strictPluginOnlyCustomization.mcp",
    "strictPluginOnlyCustomization.skills",
    "wslInheritsWindowsSettings",
];

/// Keys the settings reference scopes to user or managed settings.
pub const USER_OR_MANAGED_SCOPE_KEYS: &[&str] = &[
    "appendPlugins",
    "askUserQuestionTimeout",
    "autoContinueAtUsageLimit",
    "autoMode",
    "autoMode.classifyAllShell",
    "axScreenReader",
    "bashEditDiffEnabled",
    "desktopSessionCleanupPeriodDays",
    "dialogExpiry",
    "feedbackDrafts",
    "footerLinksRegexes",
    "forceLoginGatewayUrl",
    "modelPicker",
    "permissions.denyInsteadOfPrompting",
    "pluginConfigs",
    "prependPlugins",
    "processWrapper",
    "sandbox.allowAppleEvents",
    "sandbox.allowNestedUserNamespaces",
    "sandbox.credentials.allowPlaintextInject",
    "sandbox.credentials.awsPairs",
    "sandbox.credentials.sigv4",
    "sandbox.filesystem.disabled",
    "sandbox.network.strictAllowlist",
    "sandbox.network.tlsTerminate",
    "sandbox.ripgrep",
    "skipAutoPermissionPrompt",
    "spellcheck",
    "sshConfigs",
    "sshHostCommands",
    "vimInsertModeRemaps",
];

/// Keys the settings reference scopes to user, local or managed settings.
pub const USER_LOCAL_OR_MANAGED_SCOPE_KEYS: &[&str] = &[
    "skipDangerousModePermissionPrompt",
    "syncClaudeAiPlugins",
    "syncClaudeAiSkills",
    "useAutoModeDuringPlan",
];

/// Keys the settings reference scopes to user settings.
pub const USER_SCOPE_KEYS: &[&str] = &["worktree.location"];

/// Keys the settings reference places in `~/.claude.json`, which no
/// `settings.json` sets.
pub const GLOBAL_CONFIG_SCOPE_KEYS: &[&str] = &[
    "autoConnectIde",
    "autoInstallIdeExtension",
    "claudeInChromeDefaultEnabled",
    "copyFullResponse",
    "copyOnSelect",
    "defaultToAgentsView",
    "diffTool",
    "externalEditorContext",
    "leftArrowOpensAgents",
    "permissionExplainerEnabled",
    "prStatusFooterEnabled",
    "teammateDefaultModel",
];

/// The scoped keys the settings reference files under Permission settings.
pub const PERMISSION_TOPIC_KEYS: &[&str] = &[
    "allowManagedPermissionRulesOnly",
    "autoMode",
    "autoMode.classifyAllShell",
    "permissions.denyInsteadOfPrompting",
    "skipAutoPermissionPrompt",
    "skipDangerousModePermissionPrompt",
    "useAutoModeDuringPlan",
];

/// The scoped keys the settings reference files under Sandbox settings.
pub const SANDBOX_TOPIC_KEYS: &[&str] = &[
    "sandbox.allowAppleEvents",
    "sandbox.allowNestedUserNamespaces",
    "sandbox.bwrapPath",
    "sandbox.credentials.allowPlaintextInject",
    "sandbox.credentials.awsPairs",
    "sandbox.credentials.sigv4",
    "sandbox.filesystem.allowManagedReadPathsOnly",
    "sandbox.filesystem.disabled",
    "sandbox.network.allowManagedDomainsOnly",
    "sandbox.network.strictAllowlist",
    "sandbox.network.tlsTerminate",
    "sandbox.ripgrep",
    "sandbox.socatPath",
];

/// `permissions.defaultMode` values that take effect only from user or
/// managed settings, though the key itself is read from any file.
pub const USER_OR_MANAGED_DEFAULT_MODES: &[&str] = &["auto", "bypassPermissions"];

/// Arrays whose `"mode": "mask"` entries take effect only from user or
/// managed settings; a project or local file's are dropped, leaving the
/// credential unprotected.
pub const USER_OR_MANAGED_MASK_ENTRIES: &[&str] =
    &["sandbox.credentials.files", "sandbox.credentials.envVars"];

/// Scoped keys a project or local file still switches off: the reference's
/// Scope line for each says a repository file can turn the feature off and
/// never on — a `false` there applies, and for `autoContinueAtUsageLimit` any
/// value does, as off.
pub const REPOSITORY_OFF_SWITCHES: &[&str] = &[
    "autoContinueAtUsageLimit",
    "bashEditDiffEnabled",
    "sandbox.allowNestedUserNamespaces",
];

/// Keys read from any file whose `true` takes effect only from user or managed
/// settings: a repository can turn the feature off and never on.
pub const USER_OR_MANAGED_TRUE_KEYS: &[&str] = &["remoteControlAtStartup"];

/// Every closed set this validator reads from the settings reference,
/// labelled. The measurement stamp digests exactly this list, so a value moved
/// from one set to another changes the digest rather than hiding in a
/// concatenation.
pub const SPEC_SETS: &[(&str, &[&str])] = &[
    ("default-mode-values", KNOWN_DEFAULT_MODE_VALUES),
    ("skill-override-values", KNOWN_SKILL_OVERRIDE_VALUES),
    ("scope-managed", MANAGED_SCOPE_KEYS),
    ("scope-user-or-managed", USER_OR_MANAGED_SCOPE_KEYS),
    (
        "scope-user-local-or-managed",
        USER_LOCAL_OR_MANAGED_SCOPE_KEYS,
    ),
    ("scope-user", USER_SCOPE_KEYS),
    ("scope-global-config", GLOBAL_CONFIG_SCOPE_KEYS),
    ("topic-permission-settings", PERMISSION_TOPIC_KEYS),
    ("topic-sandbox-settings", SANDBOX_TOPIC_KEYS),
    (
        "default-modes-user-or-managed",
        USER_OR_MANAGED_DEFAULT_MODES,
    ),
    ("mask-entries-user-or-managed", USER_OR_MANAGED_MASK_ENTRIES),
    ("repository-off-switches", REPOSITORY_OFF_SWITCHES),
    ("true-user-or-managed", USER_OR_MANAGED_TRUE_KEYS),
];

wire_enum! {
    /// The settings file a validation reads, which decides the keys and
    /// values that take effect there ([`ScopeClass::honors`]). The caller
    /// names it: the path cannot, since a home directory or a file name is
    /// no proof of which file the runtime read.
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

wire_enum! {
    /// The files a key takes effect from, as the Scope column of
    /// `/en/settings-reference` names them. A key the reference lists as
    /// `Any file` belongs to no class.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ScopeClass {
        Managed => "managed",
        UserOrManaged => "user-or-managed",
        UserLocalOrManaged => "user-local-or-managed",
        User => "user",
        /// `~/.claude.json`, which `/config` writes: no settings file.
        GlobalConfig => "global-config",
    }
}

impl ScopeClass {
    /// Whether a key of this class takes effect from a file of `scope`.
    pub fn honors(self, scope: SettingsScope) -> bool {
        use SettingsScope::{Local, Managed, User};
        match self {
            Self::Managed => scope == Managed,
            Self::UserOrManaged => matches!(scope, User | Managed),
            Self::UserLocalOrManaged => matches!(scope, User | Local | Managed),
            Self::User => scope == User,
            Self::GlobalConfig => false,
        }
    }

    /// The keys the reference gives this class.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Managed => MANAGED_SCOPE_KEYS,
            Self::UserOrManaged => USER_OR_MANAGED_SCOPE_KEYS,
            Self::UserLocalOrManaged => USER_LOCAL_OR_MANAGED_SCOPE_KEYS,
            Self::User => USER_SCOPE_KEYS,
            Self::GlobalConfig => GLOBAL_CONFIG_SCOPE_KEYS,
        }
    }

    /// The files this class names, as a finding states them.
    fn files(self) -> &'static str {
        match self {
            Self::Managed => "managed settings",
            Self::UserOrManaged => "user or managed settings",
            Self::UserLocalOrManaged => "user, local or managed settings",
            Self::User => "user settings",
            Self::GlobalConfig => "~/.claude.json",
        }
    }

    /// Where to move a key of this class.
    fn destination(self) -> &'static str {
        match self {
            Self::Managed => "deploy it in managed settings",
            Self::UserOrManaged => "move it to ~/.claude/settings.json or managed settings",
            Self::UserLocalOrManaged => {
                "move it to .claude/settings.local.json, ~/.claude/settings.json or managed \
                 settings"
            }
            Self::User => "move it to ~/.claude/settings.json",
            Self::GlobalConfig => "set it with /config, which writes ~/.claude.json",
        }
    }
}

/// Whether a key that takes effect elsewhere leaves the file stating what a
/// session may do that it does not: an organization policy, or a key the
/// reference files under Permission or Sandbox settings, whether it reads as a
/// protection or as a permission. Any other such key sets a preference, and
/// its absence changes how a session looks, not what it may do.
fn governs_access(key: &str, class: ScopeClass) -> bool {
    class == ScopeClass::Managed
        || PERMISSION_TOPIC_KEYS.contains(&key)
        || SANDBOX_TOPIC_KEYS.contains(&key)
}

/// The value at a dotted key path, each segment an object key.
fn lookup<'a>(value: &'a Value, dotted: &str) -> Option<&'a Value> {
    dotted
        .split('.')
        .try_fold(value, |value, segment| value.as_object()?.get(segment))
}

/// A finding for each key the file sets that takes effect only from other
/// files, unless it switches off a [`REPOSITORY_OFF_SWITCHES`] key. A key
/// under one already reported is not reported again.
fn ignored_keys(parsed: &Value, path: &Path, scope: SettingsScope) -> Vec<Finding> {
    let mut scoped: Vec<(&str, ScopeClass)> = ScopeClass::ALL
        .iter()
        .filter(|class| !class.honors(scope))
        .flat_map(|&class| class.keys().iter().map(move |&key| (key, class)))
        .collect();
    scoped.sort_unstable_by_key(|&(key, _)| key);
    let mut reported: Vec<&str> = Vec::new();
    let mut findings = Vec::new();
    for (key, class) in scoped {
        let under_reported = reported.iter().any(|parent| {
            key.strip_prefix(parent)
                .is_some_and(|rest| rest.starts_with('.'))
        });
        if under_reported {
            continue;
        }
        let Some(value) = lookup(parsed, key) else {
            continue;
        };
        let off_switch = REPOSITORY_OFF_SWITCHES.contains(&key);
        if off_switch && *value == Value::Bool(false) {
            continue;
        }
        reported.push(key);
        let effect = if off_switch {
            "it can only turn the feature off"
        } else {
            "it is ignored"
        };
        findings.push(Finding {
            slug: "settings-ignored-key".into(),
            severity: if governs_access(key, class) {
                Severity::Major
            } else {
                Severity::Minor
            },
            location: Location::file(path.to_path_buf()),
            message: format!(
                "'{key}' takes effect only from {}; in this {} settings file {effect}",
                class.files(),
                scope.as_str()
            ),
            hint: Some(class.destination().into()),
        });
    }
    findings
}

/// A finding for a value that takes effect only from user or managed
/// settings, at `severity`: Major for a permission mode or a masked
/// credential, which decide what a session may do, and Minor for a preference.
fn ignored_value(
    path: &Path,
    scope: SettingsScope,
    what: &str,
    hint: &str,
    severity: Severity,
) -> Finding {
    Finding {
        slug: "settings-ignored-value".into(),
        severity,
        location: Location::file(path.to_path_buf()),
        message: format!(
            "{what} takes effect only from {}; in this {} settings file it is ignored",
            ScopeClass::UserOrManaged.files(),
            scope.as_str()
        ),
        hint: Some(hint.into()),
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
    ("matcher-wide-charset-events", KNOWN_MATCHER_EVENTS),
    ("tool-query-events", TOOL_QUERY_EVENTS),
];

/// The events whose matchers take the wide exact-string charset
/// (`[a-zA-Z0-9_|, -]`) and split on `|` and `,` alike. Every other event
/// takes `[a-zA-Z0-9_|]` split on `|` only.
///
/// /en/hooks names the two narrow events among those with matcher support,
/// `FileChanged` and `StopFailure`, so this set is the rest of its matcher
/// table; the dispatcher's own set literal agreed at 2.1.325. It is distinct
/// from the dispatcher's query-field switch.
pub const KNOWN_MATCHER_EVENTS: &[&str] = &[
    "ConfigChange",
    "DirectoryAdded",
    "Elicitation",
    "ElicitationResult",
    "InstructionsLoaded",
    "Notification",
    "PermissionDenied",
    "PermissionRequest",
    "PostCompact",
    "PostModelSwitch",
    "PostToolUse",
    "PostToolUseFailure",
    "PreCompact",
    "PreModelSwitch",
    "PreToolUse",
    "SessionEnd",
    "SessionStart",
    "Setup",
    "SubagentStart",
    "SubagentStop",
    "UserPromptExpansion",
];

/// The events whose matcher query is a tool name — the only events where a
/// token like `mcp__server__tool` is what the dispatcher compares. On an
/// event outside the dispatcher's query switch it consults no matcher at
/// all and every hook fires, so a matcher there is ignored, not dead. The
/// tool-name row of /en/hooks's matcher table.
pub const TOOL_QUERY_EVENTS: &[&str] = &[
    "PermissionDenied",
    "PermissionRequest",
    "PostToolUse",
    "PostToolUseFailure",
    "PreToolUse",
];

/// The alternatives an exact-string matcher compares by membership, or
/// `None` where the dispatcher reads the matcher another way: absent, empty
/// and `*` match everything, and a matcher outside its event's charset is an
/// unanchored regex over the query. Tokens are trimmed and empty
/// alternatives dropped, as the dispatcher drops them — an empty token
/// narrows nothing and widens nothing.
///
/// Two alias layers are deliberately unmodelled. The session-level
/// expansion is additive and unknowable from a settings file, so omitting
/// it only under-reports what a matcher covers. The static substitution
/// (`Task` → `Agent` and eleven siblings) CAN invert a verdict in general —
/// a `Task` matcher never fires for a tool literally named Task — but no
/// key or value of that map is a session source or begins with `mcp__`, so
/// neither consumer of this function can reach a token it rewrites. A
/// consumer comparing tool names outside the `mcp__` namespace must model
/// it before trusting membership.
pub(crate) fn exact_matcher_tokens<'a>(event: &str, matcher: &'a str) -> Option<Vec<&'a str>> {
    if matcher.is_empty() || matcher == "*" {
        return None;
    }
    let wide = KNOWN_MATCHER_EVENTS.contains(&event);
    let in_charset = |c: char| {
        c.is_ascii_alphanumeric() || c == '_' || c == '|' || (wide && matches!(c, ',' | ' ' | '-'))
    };
    if !matcher.chars().all(in_charset) {
        return None;
    }
    Some(
        matcher
            .split(|c| c == '|' || (wide && c == ','))
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .collect(),
    )
}

#[derive(Default)]
pub struct SettingsValidator;

impl SettingsValidator {
    pub fn new() -> Self {
        Self
    }

    pub fn validate_file(&self, path: &Path, scope: SettingsScope) -> Vec<Finding> {
        let contents = match super::read_text(path) {
            Ok(contents) => contents,
            Err(finding) => return vec![finding],
        };
        self.validate_text(&contents, path, scope)
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
                    if let RuleEffect::Inert(inert) = parsed.effect(direction) {
                        findings.push(Finding {
                            slug: "settings-inert-permission-rule".into(),
                            severity: Severity::Major,
                            location: Location::file(path.to_path_buf()),
                            message: format!(
                                "'{rule}' in permissions.{array} is never consulted — {}",
                                inert.reason_text()
                            ),
                            hint: Some(inert.hint()),
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
                    });
                }
            }
        }

        // permissions.defaultMode: closed-enum value check
        if let Some(mode) = parsed.pointer("/permissions/defaultMode") {
            match mode.as_str() {
                Some(s) if KNOWN_DEFAULT_MODE_VALUES.contains(&s) => {
                    if USER_OR_MANAGED_DEFAULT_MODES.contains(&s)
                        && !ScopeClass::UserOrManaged.honors(scope)
                    {
                        findings.push(ignored_value(
                            path,
                            scope,
                            &format!("permissions.defaultMode \"{s}\""),
                            "set it in ~/.claude/settings.json or managed settings, or pass \
                             --permission-mode for one session",
                            Severity::Major,
                        ));
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
                    });
                }
            }
        }

        findings.extend(ignored_keys(&parsed, path, scope));
        if !ScopeClass::UserOrManaged.honors(scope) {
            for array in USER_OR_MANAGED_MASK_ENTRIES {
                let masks = lookup(&parsed, array)
                    .and_then(Value::as_array)
                    .is_some_and(|entries| {
                        entries
                            .iter()
                            .any(|entry| entry.get("mode").and_then(Value::as_str) == Some("mask"))
                    });
                if masks {
                    findings.push(ignored_value(
                        path,
                        scope,
                        &format!("a \"mask\" entry in {array}"),
                        "move the entry to ~/.claude/settings.json or managed settings; from \
                         this file only a \"deny\" entry protects the credential",
                        Severity::Major,
                    ));
                }
            }
            for key in USER_OR_MANAGED_TRUE_KEYS {
                if lookup(&parsed, key) == Some(&Value::Bool(true)) {
                    findings.push(ignored_value(
                        path,
                        scope,
                        &format!("'{key}': true"),
                        "set it in ~/.claude/settings.json or managed settings; from this file \
                         only a `false` takes effect",
                        Severity::Minor,
                    ));
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
                        });
                    }
                } else {
                    findings.push(Finding {
                        slug: "settings-skill-override-invalid".into(),
                        severity: Severity::Major,
                        location: Location::file(path.to_path_buf()),
                        message: format!("skillOverrides['{skill_name}'] must be a string"),
                        hint: Some("set to on, name-only, user-invocable-only, or off".into()),
                    });
                }
            }
        }

        if scope == SettingsScope::Project {
            for pattern in crate::always_loaded::unread_excludes(&parsed) {
                findings.push(Finding {
                    slug: "settings-exclude-unread".into(),
                    severity: Severity::Info,
                    location: Location::file(path.to_path_buf()),
                    message: format!(
                        "claudeMdExcludes pattern '{pattern}' is written in syntax harnex does \
                         not read as Claude Code does, so a memory file its glob excludes still \
                         counts toward the always-loaded budget and still has its claims checked"
                    ),
                    hint: Some(
                        "write it with literal paths, `*`, `**`, `?`, `[…]` classes and `{a,b}` \
                         groups, which harnex reads as Claude Code does, or accept the file \
                         counted"
                            .into(),
                    ),
                });
            }
        }

        findings
    }

    /// Hold a `SessionStart` matcher's alternatives to the documented source
    /// set.
    ///
    /// [`exact_matcher_tokens`] decides what an alternative is: a matcher
    /// outside the event's charset is a regex (`.*` matches sources this set
    /// cannot enumerate, so membership is not asked of it), and within it the
    /// dispatcher splits on `|` and `,`, trims, and drops empty tokens — so
    /// `startup, resume` fires for both and is not flagged, while
    /// `startup resume` and `startup-resume` survive splitting as single
    /// tokens that equal no source and fire for nothing, which is precisely
    /// the defect this check exists to name.
    ///
    /// A dead alternative is otherwise silent: it matches no session and the
    /// hook simply never fires for it, which reads as the hook working because
    /// the other alternatives still do.
    fn session_start_sources(&self, entries: &Value, path: &Path) -> Vec<Finding> {
        let mut findings = Vec::new();
        let Some(entries) = entries.as_array() else {
            return findings;
        };
        for entry in entries {
            let Some(matcher) = entry.get("matcher").and_then(Value::as_str) else {
                continue;
            };
            let Some(tokens) = exact_matcher_tokens("SessionStart", matcher) else {
                continue;
            };
            for unknown in tokens
                .into_iter()
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
                });
            }
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::{PERMISSION_TOPIC_KEYS, SANDBOX_TOPIC_KEYS, SPEC_SETS, ScopeClass};

    /// A misspelled entry in either measured set would silently take the
    /// wrong arm — the narrow charset, or no MCP judgment at all — so both
    /// are held inside the event vocabulary the validator already owns.
    #[test]
    fn the_measured_matcher_sets_name_known_events_only() {
        for event in super::KNOWN_MATCHER_EVENTS {
            assert!(
                super::KNOWN_HOOK_EVENTS.contains(event),
                "'{event}' is not a known hook event"
            );
        }
        for event in super::TOOL_QUERY_EVENTS {
            assert!(
                super::KNOWN_MATCHER_EVENTS.contains(event),
                "'{event}' takes the wide charset it is judged under"
            );
        }
    }

    /// The reference gives each key one Scope value and one Topic, so a key
    /// copied into two classes, or a topic key copied from no class, is a
    /// transcription slip — and each class is stamped as its own set.
    #[test]
    fn each_scoped_key_has_one_class_and_each_class_is_stamped() {
        let mut seen = std::collections::BTreeMap::new();
        for class in ScopeClass::ALL {
            let label = format!("scope-{}", class.as_str());
            assert!(
                SPEC_SETS
                    .iter()
                    .any(|(set, keys)| *set == label && *keys == class.keys()),
                "{label} is not stamped"
            );
            for key in class.keys() {
                if let Some(other) = seen.insert(*key, class.as_str()) {
                    panic!("'{key}' is in both {other} and {}", class.as_str());
                }
            }
        }
        for key in PERMISSION_TOPIC_KEYS.iter().chain(SANDBOX_TOPIC_KEYS) {
            assert!(
                seen.contains_key(key),
                "topic key '{key}' has no scope class"
            );
        }
    }
}
