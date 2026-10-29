//! Validator for `.claude/output-styles/*.md`.
//!
//! An output style rewrites the system prompt for every turn of a session
//! (<https://code.claude.com/docs/en/output-styles>). The runtime reads its two
//! flags as [`frontmatter::flag`] does, and a value it reads as neither true
//! nor false leaves the flag at its `false` default with no error anywhere.
//! For `keep-coding-instructions` on the full system prompt, that drops Claude
//! Code's built-in engineering instructions for the whole session.
//!
//! Checks:
//! - Frontmatter, where present, parses as YAML.
//! - `keep-coding-instructions` and `force-for-plugin` read as a flag.
//! - Opt-in via `OutputStylesPolicy.reject_unknown_keys`: a key outside
//!   `KNOWN_OUTPUT_STYLE_KEYS`.
//!
//! ## What this module refuses to do
//!
//! - Never require frontmatter or `name`. A style without them loads under
//!   its file name with every flag at its default, which is what a style that
//!   only changes how Claude talks asks for.
//! - Never judge the body. It is prompt text, and no documented budget bounds
//!   it.

use std::path::Path;

use serde::Deserialize;

use crate::config::OutputStylesPolicy;
use crate::envelope::{Finding, Location, Severity};
use crate::validate::frontmatter::{self, FLAG_SPELLINGS};

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

    pub fn validate_file(&self, path: &Path) -> Vec<Finding> {
        let contents = match super::read_text(path) {
            Ok(contents) => contents,
            Err(finding) => return vec![finding],
        };
        self.validate_text(&contents, path)
    }

    pub fn validate_text(&self, content: &str, path: &Path) -> Vec<Finding> {
        let mut findings = Vec::new();

        let fm = match frontmatter::parse(content, path) {
            Ok(Some(fm)) => fm,
            Ok(None) => return findings,
            Err(e) => {
                let hint = e.hint().map(String::from);
                findings.push(Finding {
                    slug: "output-style-frontmatter-malformed".into(),
                    severity: Severity::Blocker,
                    location: Location::line(path.to_path_buf(), 1),
                    message: e.to_string(),
                    hint,
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
                    });
                }
            }
        }

        for (value, field, consequence) in [
            (
                &parsed.keep_coding_instructions,
                "keep-coding-instructions",
                "on the full system prompt the style drops Claude Code's built-in engineering \
                 instructions (the shorter prompt carries none to keep)",
            ),
            (
                &parsed.force_for_plugin,
                "force-for-plugin",
                "a plugin's style stays opt-in (outside a plugin the key does nothing)",
            ),
        ] {
            if let Some(v) = value
                && frontmatter::flag(v).is_none()
            {
                findings.push(Finding {
                    slug: format!("output-style-{field}-invalid"),
                    severity: Severity::Major,
                    location: Location::line(path.to_path_buf(), line),
                    message: format!(
                        "{field} must be a flag ({FLAG_SPELLINGS}); the runtime reads any other \
                         value as unset, so {consequence}"
                    ),
                    hint: Some(format!("set `{field}: true` or `{field}: false`")),
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

    fn validate_path(&self, path: &Path) -> Vec<Finding> {
        self.validate_file(path)
    }
}
