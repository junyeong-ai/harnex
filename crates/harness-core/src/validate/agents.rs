//! Validator for `.claude/agents/*.md`.
//!
//! Subagent definitions per <https://code.claude.com/docs/en/sub-agents>.
//! The body is the agent's system prompt; everything checked here is
//! frontmatter shape.
//!
//! Checks:
//! - Frontmatter present and parses as YAML.
//! - `name` and `description` present; `name` carries no character the
//!   `agent_type` surface cannot address.
//! - `permissionMode`, `effort`, `isolation`, `memory`, `color` are closed
//!   sets in the spec — a value outside one is silently ignored at load.
//! - `maxTurns` is a positive integer; `background` is a boolean.
//! - `tools` / `disallowedTools` / `skills` / `mcpServers` carry a shape the
//!   loader can read.
//! - `hooks` keys are in `KNOWN_HOOK_EVENTS`.
//! - Opt-in via `AgentsPolicy.reject_unknown_keys`: a key outside
//!   `KNOWN_AGENT_KEYS`.
//!
//! ## What this module refuses to do
//!
//! - Never check `name` against the filename. The spec resolves an agent by
//!   its declared `name`, not by its path — unlike a skill, whose command
//!   comes from the directory. Requiring the two to match would flag a
//!   correct definition.
//! - Never resolve `agent` / `model` references. An agent name reaches the
//!   loader from project, user, plugin, and CLI scopes at once, and only the
//!   project scope is on disk here; a "does not resolve" finding would fire
//!   on every user-scope agent. `model` is free-form for the same reason it
//!   is in the skill validator: aliases and full ids are both valid and the
//!   set moves with the vendor.
//! - Never judge the body. It is a system prompt, and no documented budget
//!   bounds it.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use crate::config::AgentsPolicy;
use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::validate::frontmatter;
use crate::validate::settings::KNOWN_HOOK_EVENTS;
use crate::validate::skills::KNOWN_EFFORT_LEVELS;

/// `name` addresses the agent as `agent_type` in hook payloads and in
/// `Agent(...)` permission rules. Uppercase, whitespace, and `:` (the plugin
/// namespace separator) make a name those surfaces cannot carry. Digits are
/// accepted because the spec bounds the character class loosely and the set
/// errs toward accepting, exactly as `KNOWN_HOOK_EVENTS` does.
static NAME_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9-]+$").expect("NAME_PATTERN"));

pub const KNOWN_PERMISSION_MODES: &[&str] = &[
    "default",
    "acceptEdits",
    "auto",
    "dontAsk",
    "bypassPermissions",
    "plan",
    "manual",
];

pub const KNOWN_ISOLATION_MODES: &[&str] = &["worktree"];

pub const KNOWN_MEMORY_SCOPES: &[&str] = &["user", "project", "local"];

pub const KNOWN_COLORS: &[&str] = &[
    "red", "blue", "green", "yellow", "purple", "orange", "pink", "cyan",
];

/// Complete Claude Code subagent frontmatter key surface (wire names).
/// Broader than `AgentFrontmatter`'s modeled fields — includes spec keys the
/// validator does not type-check (`initialPrompt`) so `reject_unknown_keys`
/// never false-positives on a valid-but-unmodeled key. Update when the
/// upstream sub-agents spec adds a key (same contract as KNOWN_HOOK_EVENTS).
pub const KNOWN_AGENT_KEYS: &[&str] = &[
    "name",
    "description",
    "tools",
    "disallowedTools",
    "model",
    "permissionMode",
    "maxTurns",
    "skills",
    "mcpServers",
    "hooks",
    "memory",
    "background",
    "effort",
    "isolation",
    "color",
    "initialPrompt",
];

/// Every closed set this validator reads from the sub-agents page, labelled.
///
/// The measurement stamp digests exactly this list, so a set that is not in it
/// is a set no stamp covers — the one list to extend when a new closed value
/// set is added, and the one place a reviewer checks that none was forgotten.
pub const SPEC_SETS: &[(&str, &[&str])] = &[
    ("agent-keys", KNOWN_AGENT_KEYS),
    ("permission-modes", KNOWN_PERMISSION_MODES),
    ("effort-levels", KNOWN_EFFORT_LEVELS),
    ("isolation-modes", KNOWN_ISOLATION_MODES),
    ("memory-scopes", KNOWN_MEMORY_SCOPES),
    ("colors", KNOWN_COLORS),
];

pub struct AgentValidator<'a> {
    policy: &'a AgentsPolicy,
}

/// Strongly typed subset of subagent frontmatter. Fields whose spec shape is
/// a union (`tools` accepts a string or a list) are parsed as
/// `yaml_serde::Value` so the check validates shape without forcing one arm.
#[derive(Debug, Deserialize, Default)]
struct AgentFrontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tools: Option<yaml_serde::Value>,
    #[serde(default, rename = "disallowedTools")]
    disallowed_tools: Option<yaml_serde::Value>,
    #[serde(default, rename = "permissionMode")]
    permission_mode: Option<String>,
    #[serde(default, rename = "maxTurns")]
    max_turns: Option<yaml_serde::Value>,
    #[serde(default)]
    skills: Option<yaml_serde::Value>,
    #[serde(default, rename = "mcpServers")]
    mcp_servers: Option<yaml_serde::Value>,
    #[serde(default)]
    hooks: Option<yaml_serde::Value>,
    #[serde(default)]
    memory: Option<String>,
    #[serde(default)]
    background: Option<yaml_serde::Value>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    isolation: Option<String>,
    #[serde(default)]
    color: Option<String>,
}

impl<'a> AgentValidator<'a> {
    pub fn new(policy: &'a AgentsPolicy) -> Self {
        Self { policy }
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

        let fm = match frontmatter::parse(content, path) {
            Ok(Some(fm)) => fm,
            Ok(None) => {
                findings.push(Finding {
                    slug: "agent-missing-frontmatter".into(),
                    severity: Severity::Blocker,
                    location: Location::line(path.to_path_buf(), 1),
                    message: "agent definition has no YAML frontmatter".into(),
                    hint: Some(
                        "open the file with a `---` fence declaring at least `name` and `description`"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
                return findings;
            }
            Err(e) => {
                let hint = e.hint().map(String::from);
                findings.push(Finding {
                    slug: "agent-frontmatter-malformed".into(),
                    severity: Severity::Blocker,
                    location: Location::line(path.to_path_buf(), 1),
                    message: e.to_string(),
                    hint,
                    auto_fixable: false,
                    fix_command: None,
                });
                return findings;
            }
        };

        let line = fm.begin_line;
        let parsed = match yaml_serde::from_str::<AgentFrontmatter>(&fm.yaml_text) {
            Ok(p) => p,
            Err(e) => {
                findings.push(Finding {
                    slug: "agent-frontmatter-yaml-invalid".into(),
                    severity: Severity::Blocker,
                    location: Location::line(path.to_path_buf(), line),
                    message: format!("yaml parse: {e}"),
                    hint: Some(
                        "fix the YAML between the `---` fences; common causes: \
                         unquoted strings with `:`, tab indentation, missing list `- ` prefix"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
                return findings;
            }
        };

        if self.policy.reject_unknown_keys
            && let Ok(yaml_serde::Value::Mapping(mapping)) =
                yaml_serde::from_str::<yaml_serde::Value>(&fm.yaml_text)
        {
            for key in mapping.keys() {
                if let Some(key) = key.as_str()
                    && !KNOWN_AGENT_KEYS.contains(&key)
                {
                    findings.push(Finding {
                        slug: "agent-unknown-frontmatter-key".into(),
                        severity: Severity::Major,
                        location: Location::line(path.to_path_buf(), line),
                        message: format!(
                            "unknown frontmatter key '{key}' is not in the Claude Code sub-agent spec; Claude Code silently ignores it"
                        ),
                        hint: Some(format!(
                            "remove it or fix the typo — known keys: {}",
                            KNOWN_AGENT_KEYS.join(", ")
                        )),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        match parsed.name.as_deref() {
            None => findings.push(Finding {
                slug: "agent-missing-name".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), line),
                message: "agent frontmatter has no `name`".into(),
                hint: Some(
                    "declare `name:` — it is the identifier hooks receive as `agent_type` and \
                     `Agent(...)` permission rules address"
                        .into(),
                ),
                auto_fixable: false,
                fix_command: None,
            }),
            Some(name) if !NAME_PATTERN.is_match(name) => findings.push(Finding {
                slug: "agent-name-shape".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), line),
                message: format!(
                    "agent name '{name}' must be lowercase letters, digits, and hyphens"
                ),
                hint: Some(
                    "rename to the `agent_type` form; `:` is reserved for the plugin namespace"
                        .into(),
                ),
                auto_fixable: false,
                fix_command: None,
            }),
            Some(_) => {}
        }

        if parsed.description.is_none() {
            findings.push(Finding {
                slug: "agent-missing-description".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), line),
                message: "agent frontmatter has no `description`".into(),
                hint: Some(
                    "declare `description:` — it is the surface Claude reads to decide delegation"
                        .into(),
                ),
                auto_fixable: false,
                fix_command: None,
            });
        }

        for (value, field, allowed) in [
            (
                &parsed.permission_mode,
                "permissionMode",
                KNOWN_PERMISSION_MODES,
            ),
            (&parsed.effort, "effort", KNOWN_EFFORT_LEVELS),
            (&parsed.isolation, "isolation", KNOWN_ISOLATION_MODES),
            (&parsed.memory, "memory", KNOWN_MEMORY_SCOPES),
            (&parsed.color, "color", KNOWN_COLORS),
        ] {
            if let Some(v) = value
                && !allowed.contains(&v.as_str())
            {
                findings.push(Finding {
                    slug: format!("agent-{}-invalid", to_kebab(field)),
                    severity: Severity::Major,
                    location: Location::line(path.to_path_buf(), line),
                    message: format!(
                        "{field} '{v}' is not valid; must be one of: {}",
                        allowed.join(", ")
                    ),
                    hint: Some(format!(
                        "set {field} to one of the documented values, or remove the field"
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }

        if let Some(v) = &parsed.max_turns
            && !v.as_u64().is_some_and(|n| n > 0)
        {
            findings.push(Finding {
                slug: "agent-max-turns-invalid".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), line),
                message: "maxTurns must be a positive integer".into(),
                hint: Some("set `maxTurns: <n>` with n ≥ 1, or remove the field".into()),
                auto_fixable: false,
                fix_command: None,
            });
        }

        if let Some(v) = &parsed.background
            && !v.is_bool()
        {
            findings.push(Finding {
                slug: "agent-background-invalid".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), line),
                message: "background must be a boolean (true or false)".into(),
                hint: Some("set `background: true` or `background: false`".into()),
                auto_fixable: false,
                fix_command: None,
            });
        }

        for (value, field) in [
            (&parsed.tools, "tools"),
            (&parsed.disallowed_tools, "disallowedTools"),
        ] {
            if let Some(v) = value {
                findings.extend(tool_list_finding(v, field, path, line));
            }
        }

        if let Some(v) = &parsed.skills
            && !v
                .as_sequence()
                .is_some_and(|s| s.iter().all(|i| i.is_string()))
        {
            findings.push(Finding {
                slug: "agent-skills-invalid".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), line),
                message: "skills must be a list of skill-name strings".into(),
                hint: Some("write `skills: [name-one, name-two]`".into()),
                auto_fixable: false,
                fix_command: None,
            });
        }

        // `mcpServers` carries either server-name strings or inline server
        // definitions, so only a scalar is provably wrong — and so is a list
        // element that could be neither. A mapping or nested list element
        // stays accepted: an inline definition is one, and guessing at its
        // interior would flag a config the spec allows.
        if let Some(v) = &parsed.mcp_servers {
            let scalar = v.is_string() || v.is_bool() || v.is_number();
            let bad_element = v.as_sequence().is_some_and(|items| {
                items
                    .iter()
                    .any(|i| i.is_bool() || i.is_number() || i.is_null())
            });
            if scalar || bad_element {
                findings.push(Finding {
                    slug: "agent-mcp-servers-invalid".into(),
                    severity: Severity::Major,
                    location: Location::line(path.to_path_buf(), line),
                    message: "mcpServers must be a list of server names or inline definitions"
                        .into(),
                    hint: Some(
                        "write `mcpServers: [server-name]` or nest the inline config".into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }

        // A `hooks` that is not a mapping has no events to check, so reading
        // only the mapping arm accepts every other shape in silence — the
        // frontmatter declares hooks and none of them are wired.
        if parsed
            .hooks
            .as_ref()
            .is_some_and(|h| !matches!(h, yaml_serde::Value::Mapping(_)))
        {
            findings.push(Finding {
                slug: "agent-hooks-invalid".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), line),
                message: "hooks must be a mapping of event name to handlers".into(),
                hint: Some(
                    "write `hooks:` with an event key beneath it, e.g. `PreToolUse:`".into(),
                ),
                auto_fixable: false,
                fix_command: None,
            });
        }

        if let Some(yaml_serde::Value::Mapping(hooks)) = &parsed.hooks {
            for key in hooks.keys() {
                if let Some(event) = key.as_str()
                    && !KNOWN_HOOK_EVENTS.contains(&event)
                {
                    findings.push(Finding {
                        slug: "agent-hooks-unknown-event".into(),
                        severity: Severity::Major,
                        location: Location::line(path.to_path_buf(), line),
                        message: format!(
                            "hook event '{event}' is not in the Claude Code spec /en/hooks"
                        ),
                        hint: Some(format!("known events: {}", KNOWN_HOOK_EVENTS.join(", "))),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        findings
    }
}

/// `tools` and `disallowedTools` are documented as comma-separated strings;
/// a YAML list of the same names is accepted for the same reason the skill
/// validator accepts both arms — erring toward acceptance keeps a valid
/// spelling from being reported as an error.
fn tool_list_finding(
    value: &yaml_serde::Value,
    field: &str,
    path: &Path,
    line: u32,
) -> Option<Finding> {
    let valid = match value.as_sequence() {
        Some(seq) => seq.iter().all(|i| i.is_string()),
        None => value.is_string(),
    };
    (!valid).then(|| Finding {
        slug: format!("agent-{}-invalid", to_kebab(field)),
        severity: Severity::Major,
        location: Location::line(path.to_path_buf(), line),
        message: format!("{field} must be a string or a list of tool-name strings"),
        hint: Some(format!("write `{field}: Read, Grep` or `[Read, Grep]`")),
        auto_fixable: false,
        fix_command: None,
    })
}

/// Wire field names are camelCase; finding slugs are kebab-case
/// (`envelope.md` § Slug grammar), so the slug is derived rather than
/// written twice.
fn to_kebab(field: &str) -> String {
    let mut out = String::with_capacity(field.len() + 2);
    for c in field.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

impl<'p> crate::validate::SurfaceValidator<'p> for AgentValidator<'p> {
    type Policy = AgentsPolicy;
    const SLUG: &'static str = "validate.agents";
    const GLOB: &'static str = ".claude/agents/**/*.md";

    fn policy(config: &'p crate::config::Config) -> Option<&'p Self::Policy> {
        config.validate.as_ref()?.agents.as_ref()
    }

    fn build(policy: &'p Self::Policy) -> Self {
        Self::new(policy)
    }

    fn validate_path(&self, path: &Path) -> Result<Vec<Finding>> {
        self.validate_file(path)
    }
}
