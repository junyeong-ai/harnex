//! Validator for git commit messages.
//!
//! Enforces closed-enum trailers under the `[validate.commit_msg]` config
//! section. Common use: validate `Nodex-Event:`, `Co-Authored-By:`,
//! `Signed-off-by:`, or any project-defined trailer where the value must
//! be one of a fixed set.
//!
//! Trailer convention follows git: lines matching
//! `^<Key>: <value>$` where Key is alpha-numeric + hyphen, appearing at
//! the end of the commit message. This validator is lenient — it accepts
//! trailer-shaped lines anywhere in the message (matches what
//! `git interpret-trailers` would surface).

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::config::CommitMsgPolicy;
use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};

static TRAILER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Za-z0-9][A-Za-z0-9-]*):\s*(.*)$").expect("TRAILER_PATTERN regex")
});

pub struct CommitMsgValidator<'a> {
    policy: &'a CommitMsgPolicy,
}

impl<'a> CommitMsgValidator<'a> {
    pub fn new(policy: &'a CommitMsgPolicy) -> Self {
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
        let seen_trailers = collect_trailers(content);

        // Check known-trailer values
        for (key, value, line_no) in &seen_trailers {
            let Some(decl) = self.policy.trailers.iter().find(|t| t.key == *key) else {
                continue;
            };
            if value.trim().is_empty() {
                findings.push(Finding {
                    slug: "commit-msg-empty-trailer".into(),
                    severity: Severity::Major,
                    location: Location::line(path.to_path_buf(), *line_no),
                    message: format!("trailer '{key}:' has empty value"),
                    hint: Some(format!(
                        "provide a non-empty value for the '{key}:' trailer"
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
                continue;
            }
            if let Some(allowed) = &decl.allowed_values
                && !allowed.iter().any(|v| v == value)
            {
                findings.push(Finding {
                    slug: "commit-msg-unknown-trailer-value".into(),
                    severity: Severity::Major,
                    location: Location::line(path.to_path_buf(), *line_no),
                    message: format!("trailer '{key}: {value}' value not in allowed set"),
                    hint: Some(format!("allowed: {}", allowed.join(", "))),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }

        // Check required-trailer presence
        for decl in &self.policy.trailers {
            if !decl.required {
                continue;
            }
            let present = seen_trailers.iter().any(|(k, _, _)| k == &decl.key);
            if !present {
                findings.push(Finding {
                    slug: "commit-msg-missing-required-trailer".into(),
                    severity: Severity::Blocker,
                    location: Location::file(path.to_path_buf()),
                    message: format!(
                        "required trailer '{}:' not found in commit message",
                        decl.key
                    ),
                    hint: Some(format!(
                        "add a trailer line: '{}: <value>'{}",
                        decl.key,
                        decl.allowed_values
                            .as_ref()
                            .map(|v| format!(" (allowed: {})", v.join(", ")))
                            .unwrap_or_default()
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }

        findings
    }
}

/// Extract every `Key: Value` trailer per git convention: trailers live
/// in the LAST contiguous block of non-empty lines, separated from the
/// body by at least one blank line. This prevents the subject line
/// (e.g., `feat: add lint`) from being mis-validated as a `feat:` trailer.
///
/// Returns `(key, value, 1-indexed line number)` tuples.
fn collect_trailers(content: &str) -> Vec<(String, String, u32)> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Walk backward from the end, collecting non-empty lines until we hit
    // a blank line. That contiguous block is the trailer paragraph.
    // Special case: a message with a single paragraph (no blank line)
    // has no trailers — the entire content is the subject.
    let mut trailer_start: Option<usize> = None;
    let mut trailer_end: Option<usize> = None;
    let mut saw_blank = false;
    for (idx, line) in lines.iter().enumerate().rev() {
        if line.trim().is_empty() {
            if trailer_end.is_some() {
                trailer_start = Some(idx + 1);
                saw_blank = true;
                break;
            }
            continue;
        }
        if trailer_end.is_none() {
            trailer_end = Some(idx);
        }
    }
    if !saw_blank {
        return Vec::new();
    }
    let Some(start) = trailer_start else {
        return Vec::new();
    };
    let Some(end) = trailer_end else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate().take(end + 1).skip(start) {
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        if let Some(cap) = TRAILER_PATTERN.captures(line) {
            let key = cap[1].to_string();
            let value = cap[2].to_string();
            out.push((key, value, idx as u32 + 1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CommitMsgTrailerDecl;
    use std::path::PathBuf;

    fn policy(trailers: Vec<CommitMsgTrailerDecl>) -> CommitMsgPolicy {
        CommitMsgPolicy { trailers }
    }

    #[test]
    fn accepts_allowed_trailer_value() {
        let p = policy(vec![CommitMsgTrailerDecl {
            key: "Nodex-Event".into(),
            allowed_values: Some(vec!["rule-promoted".into(), "rule-demoted".into()]),
            required: false,
        }]);
        let msg = "feat: x\n\nbody\n\nNodex-Event: rule-promoted\n";
        let findings = CommitMsgValidator::new(&p).validate_text(msg, &PathBuf::from("c"));
        assert!(findings.is_empty(), "unexpected: {findings:?}");
    }

    #[test]
    fn rejects_unknown_trailer_value() {
        let p = policy(vec![CommitMsgTrailerDecl {
            key: "Nodex-Event".into(),
            allowed_values: Some(vec!["rule-promoted".into()]),
            required: false,
        }]);
        let msg = "feat: x\n\nNodex-Event: made-up-value\n";
        let findings = CommitMsgValidator::new(&p).validate_text(msg, &PathBuf::from("c"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].slug, "commit-msg-unknown-trailer-value");
    }

    #[test]
    fn presence_only_trailer_accepts_any_value() {
        let p = policy(vec![CommitMsgTrailerDecl {
            key: "Co-Authored-By".into(),
            allowed_values: None,
            required: false,
        }]);
        let msg = "feat: x\n\nCo-Authored-By: Claude <noreply@anthropic.com>\n";
        let findings = CommitMsgValidator::new(&p).validate_text(msg, &PathBuf::from("c"));
        assert!(findings.is_empty(), "unexpected: {findings:?}");
    }

    #[test]
    fn presence_only_trailer_rejects_empty_value() {
        let p = policy(vec![CommitMsgTrailerDecl {
            key: "Co-Authored-By".into(),
            allowed_values: None,
            required: false,
        }]);
        let msg = "feat: x\n\nCo-Authored-By:   \n";
        let findings = CommitMsgValidator::new(&p).validate_text(msg, &PathBuf::from("c"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].slug, "commit-msg-empty-trailer");
    }

    #[test]
    fn required_trailer_missing_is_blocker() {
        let p = policy(vec![CommitMsgTrailerDecl {
            key: "Nodex-Event".into(),
            allowed_values: Some(vec!["rule-promoted".into()]),
            required: true,
        }]);
        let msg = "feat: x\n\nbody only\n";
        let findings = CommitMsgValidator::new(&p).validate_text(msg, &PathBuf::from("c"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].slug, "commit-msg-missing-required-trailer");
        assert_eq!(findings[0].severity, Severity::Blocker);
    }

    #[test]
    fn unknown_trailer_keys_are_ignored() {
        // The validator only cares about trailers declared in policy.
        // Unknown trailer keys (like a casual "Note: ..." sentence at the
        // start) don't produce findings.
        let p = policy(vec![CommitMsgTrailerDecl {
            key: "Nodex-Event".into(),
            allowed_values: Some(vec!["rule-promoted".into()]),
            required: false,
        }]);
        let msg = "feat: x\n\nReviewed-by: someone\nSigned-off-by: someone-else\n";
        let findings = CommitMsgValidator::new(&p).validate_text(msg, &PathBuf::from("c"));
        assert!(findings.is_empty(), "unexpected: {findings:?}");
    }

    #[test]
    fn subject_line_is_not_mistaken_for_trailer() {
        // Subject "feat: add lint" must NOT be parsed as `feat:` trailer
        // even if the policy happens to declare a "feat" trailer.
        let p = policy(vec![CommitMsgTrailerDecl {
            key: "feat".into(),
            allowed_values: Some(vec!["allowed-only".into()]),
            required: false,
        }]);
        let msg = "feat: add lint\n\nbody paragraph\n\nNodex-Event: rule-promoted\n";
        let findings = CommitMsgValidator::new(&p).validate_text(msg, &PathBuf::from("c"));
        assert!(findings.is_empty(), "unexpected: {findings:?}");
    }

    #[test]
    fn single_paragraph_message_has_no_trailers() {
        // A message with only a subject (no blank line) has no trailer block.
        let p = policy(vec![CommitMsgTrailerDecl {
            key: "Nodex-Event".into(),
            allowed_values: Some(vec!["rule-promoted".into()]),
            required: true,
        }]);
        let msg = "feat: subject-only commit";
        let findings = CommitMsgValidator::new(&p).validate_text(msg, &PathBuf::from("c"));
        // Required trailer not found — exactly one Blocker
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].slug, "commit-msg-missing-required-trailer");
    }

    #[test]
    fn body_paragraph_with_colon_lines_is_not_trailer() {
        // Body prose like "Note: be careful" appears mid-message.
        // It must NOT be parsed as a trailer.
        let p = policy(vec![CommitMsgTrailerDecl {
            key: "Note".into(),
            allowed_values: Some(vec!["only-this".into()]),
            required: false,
        }]);
        let msg =
            "feat: x\n\nNote: this is body prose, not a trailer\n\nNodex-Event: rule-promoted\n";
        let p2 = policy(vec![
            CommitMsgTrailerDecl {
                key: "Note".into(),
                allowed_values: Some(vec!["only-this".into()]),
                required: false,
            },
            CommitMsgTrailerDecl {
                key: "Nodex-Event".into(),
                allowed_values: Some(vec!["rule-promoted".into()]),
                required: false,
            },
        ]);
        let _ = p;
        let findings = CommitMsgValidator::new(&p2).validate_text(msg, &PathBuf::from("c"));
        // "Note: this is body prose" must NOT trigger (it's in body block,
        // not trailer block). Nodex-Event in trailer block is valid → 0 findings.
        assert!(findings.is_empty(), "unexpected: {findings:?}");
    }

    #[test]
    fn indented_lines_are_not_trailers() {
        // "  Note: foo" with leading whitespace is body prose, not a trailer.
        let p = policy(vec![CommitMsgTrailerDecl {
            key: "Note".into(),
            allowed_values: Some(vec!["allowed-only".into()]),
            required: false,
        }]);
        let msg = "feat: x\n\nbody\n  Note: prose with colon\n";
        let findings = CommitMsgValidator::new(&p).validate_text(msg, &PathBuf::from("c"));
        assert!(findings.is_empty(), "unexpected: {findings:?}");
    }
}
