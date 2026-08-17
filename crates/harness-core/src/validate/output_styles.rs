//! Validator for `.claude/output-styles/*.md`.
//!
//! An output style rewrites the system prompt for every turn of a session
//! (<https://code.claude.com/docs/en/output-styles>), which makes its two
//! booleans unusually expensive to get wrong: `keep-coding-instructions`
//! defaults to `false`, so a style that meant to keep Claude's software
//! engineering instructions and wrote the flag in a shape YAML reads as a
//! string silently drops them for the whole session, with no error anywhere.
//!
//! Checks:
//! - Frontmatter present and parses as YAML.
//! - `keep-coding-instructions` and `force-for-plugin` are booleans.
//! - Opt-in via `OutputStylesPolicy.reject_unknown_keys`: a key outside
//!   `KNOWN_OUTPUT_STYLE_KEYS`.
//!
//! ## What this module refuses to do
//!
//! - Never require `name`. The spec falls back to the file name, so demanding
//!   the field would flag a style that loads exactly as its author intended.
//! - Never judge the body. It is prompt text, and no documented budget bounds
//!   it.

use std::path::Path;

use serde::Deserialize;

use crate::config::OutputStylesPolicy;
use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::validate::frontmatter;

/// Complete output-style frontmatter key surface (wire names).
pub const KNOWN_OUTPUT_STYLE_KEYS: &[&str] = &[
    "name",
    "description",
    "keep-coding-instructions",
    "force-for-plugin",
];

/// Every closed set this validator reads from the output-styles page.
pub const SPEC_SETS: &[(&str, &[&str])] = &[("output-style-keys", KNOWN_OUTPUT_STYLE_KEYS)];

pub struct OutputStyleValidator<'a> {
    policy: &'a OutputStylesPolicy,
}

#[derive(Debug, Deserialize, Default)]
struct OutputStyleFrontmatter {
    #[serde(default, rename = "keep-coding-instructions")]
    keep_coding_instructions: Option<yaml_serde::Value>,
    #[serde(default, rename = "force-for-plugin")]
    force_for_plugin: Option<yaml_serde::Value>,
}

impl<'a> OutputStyleValidator<'a> {
    pub fn new(policy: &'a OutputStylesPolicy) -> Self {
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
                    slug: "output-style-missing-frontmatter".into(),
                    severity: Severity::Blocker,
                    location: Location::line(path.to_path_buf(), 1),
                    message: "output style has no YAML frontmatter".into(),
                    hint: Some(
                        "open the file with a `---` fence; without one the style carries no \
                         `keep-coding-instructions` and drops Claude's engineering instructions"
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
                    slug: "output-style-frontmatter-malformed".into(),
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
        let parsed = match yaml_serde::from_str::<OutputStyleFrontmatter>(&fm.yaml_text) {
            Ok(p) => p,
            Err(e) => {
                findings.push(Finding {
                    slug: "output-style-frontmatter-yaml-invalid".into(),
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
                    && !KNOWN_OUTPUT_STYLE_KEYS.contains(&key)
                {
                    findings.push(Finding {
                        slug: "output-style-unknown-frontmatter-key".into(),
                        severity: Severity::Major,
                        location: Location::line(path.to_path_buf(), line),
                        message: format!(
                            "unknown frontmatter key '{key}' is not in the Claude Code output-style spec; Claude Code silently ignores it"
                        ),
                        hint: Some(format!(
                            "remove it or fix the typo — known keys: {}",
                            KNOWN_OUTPUT_STYLE_KEYS.join(", ")
                        )),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }

        for (value, field, consequence) in [
            (
                &parsed.keep_coding_instructions,
                "keep-coding-instructions",
                "the default is false, so a non-boolean drops Claude's engineering instructions \
                 for the whole session",
            ),
            (
                &parsed.force_for_plugin,
                "force-for-plugin",
                "a non-boolean leaves the style opt-in when it was meant to apply automatically",
            ),
        ] {
            if let Some(v) = value
                && !v.is_bool()
            {
                findings.push(Finding {
                    slug: format!("output-style-{field}-invalid"),
                    severity: Severity::Major,
                    location: Location::line(path.to_path_buf(), line),
                    message: format!("{field} must be a boolean — {consequence}"),
                    hint: Some(format!("write `{field}: true` unquoted")),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }

        findings
    }
}

impl<'p> crate::validate::SurfaceValidator<'p> for OutputStyleValidator<'p> {
    type Policy = OutputStylesPolicy;
    const SLUG: &'static str = "validate.output_styles";
    const GLOB: &'static str = ".claude/output-styles/*.md";

    fn policy(config: &'p crate::config::Config) -> Option<&'p Self::Policy> {
        config.validate.as_ref()?.output_styles.as_ref()
    }

    fn build(policy: &'p Self::Policy) -> Self {
        Self::new(policy)
    }

    fn validate_path(&self, path: &Path) -> Result<Vec<Finding>> {
        self.validate_file(path)
    }
}
