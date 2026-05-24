//! Validator for `.claude/rules/*.md`.
//!
//! Checks:
//! - Line count ≤ `max_lines` (default 200 per Claude Code memory spec).
//! - `paths:` frontmatter present unless the rule slug is in
//!   `always_loaded_slugs` (e.g., constitution).
//! - Frontmatter parses as YAML.

use std::path::Path;

use serde::Deserialize;

use crate::config::RulesPolicy;
use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::validate::frontmatter;

pub struct RuleValidator<'a> {
    policy: &'a RulesPolicy,
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
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let total_lines = content.lines().count();
        if total_lines > self.policy.max_lines {
            findings.push(Finding {
                slug: "rule-too-long".into(),
                severity: Severity::Major,
                location: Location::file(path.to_path_buf()),
                message: format!(
                    "{total_lines} lines exceeds max_lines={}",
                    self.policy.max_lines
                ),
                hint: Some("split the rule or move detail to a referenced file".into()),
                auto_fixable: false,
                fix_command: None,
            });
        }

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

        let always_loaded = self.policy.always_loaded_slugs.iter().any(|s| s == &slug);
        match &fm {
            None => {
                if !always_loaded {
                    findings.push(Finding {
                        slug: "rule-missing-paths-frontmatter".into(),
                        severity: Severity::Major,
                        location: Location::file(path.to_path_buf()),
                        message: "rule has no frontmatter and is not always-loaded".into(),
                        hint: Some(
                            "add `paths:` frontmatter or list the slug in [validate.rules].always_loaded_slugs".into(),
                        ),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
            Some(fm) => match yaml_serde::from_str::<RuleFrontmatter>(&fm.yaml_text) {
                Ok(parsed) => {
                    if parsed.paths.is_none() && !always_loaded {
                        findings.push(Finding {
                            slug: "rule-missing-paths-frontmatter".into(),
                            severity: Severity::Major,
                            location: Location::line(path.to_path_buf(), fm.begin_line),
                            message: "frontmatter lacks `paths:` key".into(),
                            hint: Some(
                                "add `paths: [...]` or list slug under always_loaded_slugs".into(),
                            ),
                            auto_fixable: false,
                            fix_command: None,
                        });
                    }
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
                }
            },
        }
        findings
    }
}
