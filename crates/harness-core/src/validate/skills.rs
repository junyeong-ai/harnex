//! Validator for `.claude/skills/*/SKILL.md`.
//!
//! Checks per Claude Code skills spec (<https://code.claude.com/docs/en/skills>):
//! - Frontmatter present and parses as YAML.
//! - Effective `name` (frontmatter or directory) matches `[a-z0-9-]{1,64}`.
//! - If frontmatter `name` declared, it equals directory name.
//! - `description + when_to_use` combined ≤ `max_description_chars`
//!   (Claude Code listing budget caps at 1536 chars).
//! - SKILL.md body line count ≤ `max_skill_md_lines` (compaction budget
//!   ≈ 5000 tokens ≈ 500 lines).
//! - `disable-model-invocation: true` recommended when description mentions
//!   side-effect verbs (`commit`, `deploy`, `delete`, `submit`, `send`, …).
//! - `user-invocable` must be boolean if present.
//! - `context` must be `"fork"` if present.
//! - `allowed-tools` must be array of strings if present.
//! - `paths` must be array of valid glob patterns if present.
//! - `hooks` keys must be in `KNOWN_HOOK_EVENTS` if present.
//! - `effort` must be one of `low|medium|high|xhigh|max` if present.
//!
//! `agent` and `model` are valid free-form fields — accepted, never flagged.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use crate::config::SkillsPolicy;
use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::validate::frontmatter;
use crate::validate::settings::KNOWN_HOOK_EVENTS;

static NAME_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9-]{1,64}$").expect("NAME_PATTERN"));

static SIDE_EFFECT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(commit|deploy|delete|submit|send|publish|release)\b")
        .expect("SIDE_EFFECT_PATTERN")
});

const KNOWN_EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// Complete Claude Code skill frontmatter key surface (wire names).
/// Broader than `SkillFrontmatter`'s modeled fields — includes spec keys
/// the validator does not type-check (argument-hint, arguments, shell) so
/// `reject_unknown_keys` never false-positives on a valid-but-unmodeled key.
/// Update when the upstream skills spec adds a key (same contract as
/// KNOWN_HOOK_EVENTS).
const KNOWN_SKILL_KEYS: &[&str] = &[
    "name",
    "description",
    "when_to_use",
    "argument-hint",
    "arguments",
    "disable-model-invocation",
    "user-invocable",
    "allowed-tools",
    "model",
    "effort",
    "context",
    "agent",
    "hooks",
    "paths",
    "shell",
];

pub struct SkillValidator<'a> {
    policy: &'a SkillsPolicy,
}

/// Strongly typed subset of skill frontmatter fields. Fields that need
/// flexible type-checking (allowed-tools, paths, hooks) are parsed as
/// `serde_yml::Value` to validate shape without requiring the strict type.
#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    when_to_use: Option<String>,
    #[serde(default, rename = "disable-model-invocation")]
    disable_model_invocation: Option<bool>,
    #[serde(default, rename = "user-invocable")]
    user_invocable: Option<serde_yml::Value>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default, rename = "allowed-tools")]
    allowed_tools: Option<serde_yml::Value>,
    #[serde(default)]
    paths: Option<serde_yml::Value>,
    #[serde(default)]
    hooks: Option<serde_yml::Value>,
    #[serde(default)]
    effort: Option<String>,
}

impl<'a> SkillValidator<'a> {
    pub fn new(policy: &'a SkillsPolicy) -> Self {
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
        let dir_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let total = content.lines().count();
        if total > self.policy.max_skill_md_lines {
            findings.push(Finding {
                slug: "skill-too-long".into(),
                severity: Severity::Major,
                location: Location::file(path.to_path_buf()),
                message: format!(
                    "{total} lines exceeds max_skill_md_lines={} (compaction budget ≈ 5000 tokens)",
                    self.policy.max_skill_md_lines
                ),
                hint: Some(
                    "move reference material to separate files under the skill directory".into(),
                ),
                auto_fixable: false,
                fix_command: None,
            });
        }

        let fm = match frontmatter::parse(content, path) {
            Ok(Some(fm)) => fm,
            Ok(None) => {
                findings.push(Finding {
                    slug: "skill-missing-frontmatter".into(),
                    severity: Severity::Blocker,
                    location: Location::file(path.to_path_buf()),
                    message: "SKILL.md has no YAML frontmatter".into(),
                    hint: Some("add `---\\nname: …\\ndescription: …\\n---` at the top".into()),
                    auto_fixable: false,
                    fix_command: None,
                });
                return findings;
            }
            Err(e) => {
                let hint = e.hint().map(String::from);
                findings.push(Finding {
                    slug: "skill-frontmatter-malformed".into(),
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

        let parsed: SkillFrontmatter = match serde_yml::from_str(&fm.yaml_text) {
            Ok(p) => p,
            Err(e) => {
                findings.push(Finding {
                    slug: "skill-frontmatter-yaml-invalid".into(),
                    severity: Severity::Blocker,
                    location: Location::line(path.to_path_buf(), fm.begin_line),
                    message: format!("yaml parse: {e}"),
                    hint: Some(
                        "fix the YAML between the `---` fences; SKILL.md requires `name`, \
                         `description`, `when_to_use` at minimum (see code.claude.com/docs/en/skills)"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
                return findings;
            }
        };

        // reject_unknown_keys (opt-in): re-parse the block as a raw mapping and
        // flag any top-level key outside the spec surface. The typed parse above
        // already succeeded, so a secondary parse failure is unexpected and is
        // skipped rather than re-reported (the primary path owns malformed YAML).
        if self.policy.reject_unknown_keys
            && let Ok(mapping) = serde_yml::from_str::<serde_yml::Mapping>(&fm.yaml_text)
        {
            for key in mapping.keys() {
                if let Some(key) = key.as_str()
                    && !KNOWN_SKILL_KEYS.contains(&key)
                {
                    findings.push(Finding {
                        slug: "skill-unknown-frontmatter-key".into(),
                        severity: Severity::Major,
                        location: Location::line(path.to_path_buf(), fm.begin_line),
                        message: format!(
                            "unknown frontmatter key '{key}' is not in the Claude Code skill spec; Claude Code silently ignores it"
                        ),
                        hint: Some(format!(
                            "remove it or fix the typo — known keys: {}",
                            KNOWN_SKILL_KEYS.join(", ")
                        )),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        let effective_name = parsed.name.clone().unwrap_or_else(|| dir_name.clone());
        if !NAME_PATTERN.is_match(&effective_name) {
            findings.push(Finding {
                slug: "skill-name-shape".into(),
                severity: Severity::Blocker,
                location: Location::line(path.to_path_buf(), fm.begin_line),
                message: format!(
                    "name '{effective_name}' must match [a-z0-9-]{{1,64}} per Claude Code spec"
                ),
                hint: Some("lowercase letters, digits, hyphens only".into()),
                auto_fixable: false,
                fix_command: None,
            });
        }
        if let Some(declared) = &parsed.name
            && declared != &dir_name
        {
            findings.push(Finding {
                slug: "skill-name-mismatch".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), fm.begin_line),
                message: format!(
                    "frontmatter name '{declared}' does not match directory '{dir_name}'"
                ),
                hint: Some("align frontmatter name with the skill directory name".into()),
                auto_fixable: false,
                fix_command: None,
            });
        }

        let desc_len = parsed.description.as_deref().map(str::len).unwrap_or(0);
        let when_len = parsed.when_to_use.as_deref().map(str::len).unwrap_or(0);
        let total_desc = desc_len + when_len;
        if total_desc > self.policy.max_description_chars {
            findings.push(Finding {
                slug: "skill-description-over-budget".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), fm.begin_line),
                message: format!(
                    "description + when_to_use = {total_desc} chars exceeds max_description_chars={} (Claude Code listing budget caps at 1536)",
                    self.policy.max_description_chars
                ),
                hint: Some("tighten description; details belong in skill body".into()),
                auto_fixable: false,
                fix_command: None,
            });
        }

        if parsed.disable_model_invocation != Some(true) {
            let desc_lower = format!(
                "{} {}",
                parsed.description.as_deref().unwrap_or(""),
                parsed.when_to_use.as_deref().unwrap_or("")
            )
            .to_lowercase();
            if SIDE_EFFECT_PATTERN.is_match(&desc_lower) {
                findings.push(Finding {
                    slug: "skill-side-effect-no-disable".into(),
                    severity: Severity::Minor,
                    location: Location::line(path.to_path_buf(), fm.begin_line),
                    message: "skill description suggests side effects but lacks `disable-model-invocation: true`".into(),
                    hint: Some(
                        "per Claude Code docs: set disable-model-invocation: true for skills with side effects".into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }

        // user-invocable: must be boolean if present
        if let Some(ref val) = parsed.user_invocable
            && !val.is_bool()
        {
            findings.push(Finding {
                slug: "skill-user-invocable-invalid".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), fm.begin_line),
                message: "user-invocable must be a boolean (true or false)".into(),
                hint: Some("set `user-invocable: true` or `user-invocable: false`".into()),
                auto_fixable: false,
                fix_command: None,
            });
        }

        // context: only allowed value is "fork"
        if let Some(ref ctx) = parsed.context
            && ctx != "fork"
        {
            findings.push(Finding {
                slug: "skill-context-invalid".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), fm.begin_line),
                message: format!("context '{ctx}' is not valid; only 'fork' is allowed"),
                hint: Some("set `context: fork` or remove the field".into()),
                auto_fixable: false,
                fix_command: None,
            });
        }

        // allowed-tools: must be an array of strings
        if let Some(ref val) = parsed.allowed_tools {
            match val.as_sequence() {
                Some(seq) => {
                    for (i, item) in seq.iter().enumerate() {
                        if !item.is_string() {
                            findings.push(Finding {
                                slug: "skill-allowed-tools-invalid".into(),
                                severity: Severity::Major,
                                location: Location::line(path.to_path_buf(), fm.begin_line),
                                message: format!("allowed-tools[{i}] is not a string"),
                                hint: Some(
                                    "each entry in allowed-tools must be a tool name string".into(),
                                ),
                                auto_fixable: false,
                                fix_command: None,
                            });
                        }
                    }
                }
                None => {
                    findings.push(Finding {
                        slug: "skill-allowed-tools-invalid".into(),
                        severity: Severity::Major,
                        location: Location::line(path.to_path_buf(), fm.begin_line),
                        message: "allowed-tools must be an array of strings".into(),
                        hint: Some("use `allowed-tools: [Bash, Read, Edit]` syntax".into()),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        // paths: must be an array of valid glob patterns
        if let Some(ref val) = parsed.paths {
            match val.as_sequence() {
                Some(seq) => {
                    for (i, item) in seq.iter().enumerate() {
                        if let Some(s) = item.as_str() {
                            if glob::Pattern::new(s).is_err() {
                                findings.push(Finding {
                                    slug: "skill-paths-invalid".into(),
                                    severity: Severity::Major,
                                    location: Location::line(path.to_path_buf(), fm.begin_line),
                                    message: format!(
                                        "paths[{i}] '{s}' is not a valid glob pattern"
                                    ),
                                    hint: Some("fix the glob syntax".into()),
                                    auto_fixable: false,
                                    fix_command: None,
                                });
                            }
                        } else {
                            findings.push(Finding {
                                slug: "skill-paths-invalid".into(),
                                severity: Severity::Major,
                                location: Location::line(path.to_path_buf(), fm.begin_line),
                                message: format!("paths[{i}] is not a string"),
                                hint: Some("each entry in paths must be a glob string".into()),
                                auto_fixable: false,
                                fix_command: None,
                            });
                        }
                    }
                }
                None => {
                    findings.push(Finding {
                        slug: "skill-paths-invalid".into(),
                        severity: Severity::Major,
                        location: Location::line(path.to_path_buf(), fm.begin_line),
                        message: "paths must be an array of glob strings".into(),
                        hint: Some("use `paths: [\"src/**/*.rs\"]` syntax".into()),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        // hooks: keys must be known hook event names
        if let Some(ref val) = parsed.hooks {
            if let Some(mapping) = val.as_mapping() {
                for key in mapping.keys() {
                    if let Some(event_name) = key.as_str()
                        && !KNOWN_HOOK_EVENTS.contains(&event_name)
                    {
                        findings.push(Finding {
                            slug: "skill-hooks-unknown-event".into(),
                            severity: Severity::Major,
                            location: Location::line(path.to_path_buf(), fm.begin_line),
                            message: format!(
                                "hook event '{event_name}' is not in the Claude Code spec /en/hooks"
                            ),
                            hint: Some(format!("known events: {}", KNOWN_HOOK_EVENTS.join(", "))),
                            auto_fixable: false,
                            fix_command: None,
                        });
                    }
                }
            } else {
                findings.push(Finding {
                    slug: "skill-hooks-invalid".into(),
                    severity: Severity::Major,
                    location: Location::line(path.to_path_buf(), fm.begin_line),
                    message: "hooks must be a mapping of event names to hook definitions".into(),
                    hint: Some("use `hooks: { PreToolUse: ... }` syntax".into()),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }

        // effort: must be one of the known levels
        if let Some(ref effort) = parsed.effort
            && !KNOWN_EFFORT_LEVELS.contains(&effort.as_str())
        {
            findings.push(Finding {
                slug: "skill-effort-invalid".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), fm.begin_line),
                message: format!(
                    "effort '{effort}' is not valid; must be one of: {}",
                    KNOWN_EFFORT_LEVELS.join(", ")
                ),
                hint: Some("set effort to low, medium, high, xhigh, or max".into()),
                auto_fixable: false,
                fix_command: None,
            });
        }

        findings
    }
}
