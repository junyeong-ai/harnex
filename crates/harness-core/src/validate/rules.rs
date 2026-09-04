//! Validator for `.claude/rules/**/*.md`.
//!
//! A rule's load scope is decided by one fact: per the Claude Code memory
//! spec a rule carrying `paths:` loads only when a matching file is read,
//! and one without it loads unconditionally. Every check here reads that
//! fact rather than a filename.
//!
//! Rules are discovered recursively per the memory spec, so a rule nested in
//! `.claude/rules/backend/` is governed exactly like a top-level one.
//!
//! Checks:
//! - Frontmatter parses as YAML.
//! - `paths:` is a glob string or a list of glob strings.
//! - `paths:` present unless the rule slug is declared in
//!   `always_loaded_slugs` (e.g., constitution).
//! - Always-loaded rules stay within `max_lines` — the spec's target for a
//!   file that enters every session's context.
//! - Path-scoped rules stay within `max_scoped_lines` when the project opts
//!   in; unbounded by default.
//!
//! ## What this module refuses to do
//!
//! - Never apply the always-loaded budget to a path-scoped rule. The 200-line
//!   target governs unconditional context cost; a rule that is read only
//!   alongside the files it governs does not spend it, and auto-failing a
//!   cohesive long rule is a false positive on a correct harness.
//! - Never assert a budget on a rule whose frontmatter will not parse. Load
//!   scope is then unknown, and a guess about which budget applies is worse
//!   than the parse error already reported.

use std::path::Path;

use serde::Deserialize;

use crate::config::RulesPolicy;
use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::validate::frontmatter;

pub struct RuleValidator<'a> {
    policy: &'a RulesPolicy,
}

/// Whether a `paths:` value is a shape Claude Code reads as globs — a
/// comma-separated string or a list of strings.
fn is_glob_shaped(value: &yaml_serde::Value) -> bool {
    match value.as_sequence() {
        Some(seq) => seq.iter().all(yaml_serde::Value::is_string),
        None => value.is_string(),
    }
}

/// Every glob a `paths:` value carries, in declaration order. The string form
/// is comma-separated per the memory spec.
fn globs(value: &yaml_serde::Value) -> Vec<String> {
    match value.as_sequence() {
        Some(seq) => seq
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::to_string)
            .collect(),
        None => value
            .as_str()
            .into_iter()
            .flat_map(|s| s.split(','))
            .map(|s| s.trim().to_string())
            .collect(),
    }
}

/// The rule's identity for `always_loaded_slugs`: its path below
/// `.claude/rules/`, extension removed. A bare file stem would let one entry
/// exempt `style.md` and `vendor/style.md` at once now that discovery is
/// recursive, and those are two different rules.
fn rule_slug(path: &Path) -> String {
    let parts: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    let below = parts
        .windows(2)
        .position(|w| w == [".claude", "rules"])
        .map(|i| parts[i + 2..].join("/"))
        .unwrap_or_else(|| parts.last().copied().unwrap_or_default().to_string());
    below
        .strip_suffix(".md")
        .unwrap_or(&below)
        .trim_start_matches('/')
        .to_string()
}

/// Whether a `paths:` value actually scopes the rule.
///
/// Presence of the key is not the question: `paths:` with no value, an empty
/// list, and a list of empty strings all carry zero globs, so Claude Code has
/// nothing to match the rule against and it is not path-scoped. Reading the
/// key alone would exempt such a rule from both the always-loaded budget and
/// the declaration requirement while it loads on every turn.
fn declares_scope(value: Option<&yaml_serde::Value>) -> bool {
    let Some(value) = value else {
        return false;
    };
    match value.as_sequence() {
        Some(seq) => seq
            .iter()
            .any(|v| v.as_str().is_some_and(|s| !s.trim().is_empty())),
        None => value.as_str().is_some_and(|s| !s.trim().is_empty()),
    }
}

#[derive(Debug, Deserialize)]
struct RuleFrontmatter {
    #[serde(default)]
    paths: Option<yaml_serde::Value>,
}

impl<'a> RuleValidator<'a> {
    pub fn new(policy: &'a RulesPolicy) -> Self {
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
        let slug = rule_slug(path);

        let fm = match frontmatter::parse(content, path) {
            Ok(v) => v,
            Err(e) => {
                let hint = e.hint().map(String::from);
                findings.push(Finding {
                    slug: "rule-frontmatter-malformed".into(),
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

        let (declares_paths, declares_governs, frontmatter_line) = match &fm {
            None => (false, false, 1),
            Some(fm) => match yaml_serde::from_str::<RuleFrontmatter>(&fm.yaml_text) {
                Ok(parsed) => {
                    let declares_governs =
                        match crate::governs::GovernsDecl::from_yaml(&fm.yaml_text) {
                            Ok(decls) => !decls.is_empty(),
                            Err(shape) => {
                                findings.push(Finding {
                                    slug: "rule-governs-invalid".into(),
                                    severity: Severity::Major,
                                    location: Location::line(path.to_path_buf(), fm.begin_line),
                                    message: shape.to_string(),
                                    hint: Some(
                                        "a declaration is `concept:` plus `live_truth:` (literal \
                                     project-relative paths, no globs) plus an optional \
                                     `decision_record:`, and a rule carries one or a list of \
                                     them — a malformed one governs nothing"
                                            .into(),
                                    ),
                                    auto_fixable: false,
                                    fix_command: None,
                                });
                                // Declared, however badly — the missing-declaration
                                // finding on top would be two findings for one defect.
                                true
                            }
                        };
                    if let Some(value) = &parsed.paths {
                        if is_glob_shaped(value) {
                            // A pattern the matcher rejects matches nothing, so
                            // the rule never loads while reading as scoped.
                            for pattern in globs(value) {
                                if glob::Pattern::new(&pattern).is_err() {
                                    findings.push(Finding {
                                        slug: "rule-paths-invalid".into(),
                                        severity: Severity::Major,
                                        location: Location::line(path.to_path_buf(), fm.begin_line),
                                        message: format!(
                                            "`paths:` glob '{pattern}' does not compile, so it \
                                             matches nothing and the rule never loads"
                                        ),
                                        hint: Some(
                                            "escape a literal bracket as `\\[`; an unreadable \
                                             pattern silently disables the rule"
                                                .into(),
                                        ),
                                        auto_fixable: false,
                                        fix_command: None,
                                    });
                                }
                            }
                        } else {
                            findings.push(Finding {
                                slug: "rule-paths-invalid".into(),
                                severity: Severity::Major,
                                location: Location::line(path.to_path_buf(), fm.begin_line),
                                message: "`paths:` must be a glob string or a list of glob strings"
                                    .into(),
                                hint: Some(
                                    "write `paths: [\"src/**/*.ts\"]`; a value Claude Code cannot \
                                     read as globs leaves the rule loading unconditionally"
                                        .into(),
                                ),
                                auto_fixable: false,
                                fix_command: None,
                            });
                        }
                    }
                    (
                        declares_scope(parsed.paths.as_ref()),
                        declares_governs,
                        fm.begin_line,
                    )
                }
                Err(e) => {
                    findings.push(Finding {
                        slug: "rule-frontmatter-yaml-invalid".into(),
                        severity: Severity::Blocker,
                        location: Location::line(path.to_path_buf(), fm.begin_line),
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
            },
        };

        if self.policy.require_governs && declares_paths && !declares_governs {
            findings.push(Finding {
                slug: "rule-missing-governs".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), frontmatter_line),
                message: "path-scoped rule declares no `governs:`".into(),
                hint: Some(
                    "declare `concept:` and `live_truth:` — what this rule is truth about — \
                     or turn off [validate.rules].require_governs"
                        .into(),
                ),
                auto_fixable: false,
                fix_command: None,
            });
        }

        if !declares_paths && !self.policy.always_loaded_slugs.iter().any(|s| s == &slug) {
            findings.push(Finding {
                slug: "rule-missing-paths-frontmatter".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), frontmatter_line),
                message: "rule has no `paths:` and is not declared always-loaded".into(),
                hint: Some(
                    "add `paths: [...]` or list the slug under [validate.rules].always_loaded_slugs"
                        .into(),
                ),
                auto_fixable: false,
                fix_command: None,
            });
        }

        let total_lines = content.lines().count();
        if declares_paths {
            if let Some(cap) = self.policy.max_scoped_lines
                && total_lines > cap
            {
                findings.push(Finding {
                    slug: "rule-too-long".into(),
                    severity: Severity::Minor,
                    location: Location::file(path.to_path_buf()),
                    message: format!(
                        "{total_lines} lines exceeds max_scoped_lines={cap} for a path-scoped rule"
                    ),
                    hint: Some(
                        "review for domain mixing; split only if the rule covers separable topics"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        } else if total_lines > self.policy.max_lines {
            findings.push(Finding {
                slug: "rule-too-long".into(),
                severity: Severity::Major,
                location: Location::file(path.to_path_buf()),
                message: format!(
                    "{total_lines} lines exceeds max_lines={} for an always-loaded rule",
                    self.policy.max_lines
                ),
                hint: Some(
                    "scope the rule with `paths:`, or move detail to a referenced file — \
                     an always-loaded rule spends its length on every session"
                        .into(),
                ),
                auto_fixable: false,
                fix_command: None,
            });
        }

        findings
    }
}

impl<'p> crate::validate::SurfaceValidator<'p> for RuleValidator<'p> {
    type Policy = RulesPolicy;
    const SLUG: &'static str = "validate.rules";
    const GLOB: &'static str = ".claude/rules/**/*.md";

    fn policy(config: &'p crate::config::Config) -> Option<&'p Self::Policy> {
        config.validate.as_ref()?.rules.as_ref()
    }

    fn build(policy: &'p Self::Policy) -> Self {
        Self::new(policy)
    }

    fn validate_path(&self, path: &Path) -> Result<Vec<Finding>> {
        self.validate_file(path)
    }
}
