//! PreToolUse floor-integrity auditor — the enforcement-surface freeze and
//! the hook-bypass tripwire.
//!
//! Two detections make the enforcement floor resist casual self-tampering:
//! a proposed Bash command that would skip the git hook stack
//! ([`bypass::detect_command_line_bypass`]), and a proposed Edit/Write to a
//! file that defines what the gates verify — the built-in floor plus the
//! `[guard.floor]` `protected_paths` a project declares. A failing gate is
//! fixed at its cause, never by weakening the gate.
//!
//! The two halves fail in deliberately opposite directions. Violation checks
//! fail open — "not proven guilty": anything that prevents evaluation is a
//! [`FloorDecision::Skip`] with a visible reason, never a block. The
//! operator's break-glass grant fails closed — "not proven authorised": an
//! unreadable override file is an absent grant ([`grant::FloorGrant`]). A
//! granted edit still surfaces as [`FloorDecision::Grant`] — the one signal
//! that the freeze was bypassed.
//!
//! This is a tripwire for the casual bypass, not a security boundary — a
//! shell is Turing-complete, so local hook enforcement is inherently
//! evadable, and a write smuggled through Bash (`sed -i`, redirection) is
//! out of scope. The authoritative backstop for a bypassed commit is the
//! project's own server-side re-run of its gates.
//!
//! ## What this module refuses to do
//!
//! - Never block on inability to evaluate — a skip carries its reason.
//! - Never read the grant from the process environment — the settings file
//!   is the single witness, and reading it live is what makes revocation
//!   immediate.
//! - Never follow symlinks — path resolution is lexical, and the residual
//!   is the tripwire's own.

pub mod bypass;
pub mod command_line;
pub mod grant;

use std::path::Path;

use serde::Serialize;

use crate::config::FloorConfig;
use grant::{FLOOR_EDIT_GRANT_KEY, FloorGrant};

/// Files every floor freezes, whatever the project declares: `harness.toml`
/// declares the floor itself, `.claude/settings.json` carries the hook
/// wiring that invokes it (the engine schema-validates that file as an
/// earlier layer), and `.claude/settings.local.json` carries the break-glass
/// override — unfrozen, the Edit tools could grant themselves the exception.
pub const BUILT_IN_PROTECTED: [&str; 3] = [
    "harness.toml",
    ".claude/settings.json",
    ".claude/settings.local.json",
];

/// One proposed tool call, judged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "decision", rename_all = "kebab-case")]
pub enum FloorDecision {
    Allow,
    /// Evaluation could not run; the write proceeds with a visible reason.
    Skip {
        reason: String,
    },
    /// A protected write allowed by the operator's standing override —
    /// surfaced, because a silent grant would hide the one signal that the
    /// freeze was bypassed.
    Grant {
        path: String,
    },
    Block {
        reason: String,
    },
}

pub struct FloorAuditor {
    protected: Vec<String>,
}

impl FloorAuditor {
    pub fn new(config: &FloorConfig) -> Self {
        Self {
            protected: config.protected_paths.clone(),
        }
    }

    /// Judge one proposed tool call. `root` is the directory `harness.toml`
    /// was found in; `tool_input` is the hook event's raw `tool_input`.
    pub fn evaluate(
        &self,
        root: &Path,
        tool_name: &str,
        tool_input: &serde_json::Value,
    ) -> FloorDecision {
        match tool_name {
            "Bash" => self.evaluate_command(tool_input),
            "Edit" | "Write" | "MultiEdit" => self.evaluate_write(root, tool_input),
            // A regex PreToolUse matcher can over-match; this dispatch — not
            // the matcher — is the authority on which tools the floor
            // evaluates.
            other => FloorDecision::Skip {
                reason: format!(
                    "unhandled tool {} — only Bash / Edit / Write / MultiEdit are evaluated",
                    if other.is_empty() { "?" } else { other }
                ),
            },
        }
    }

    fn evaluate_command(&self, tool_input: &serde_json::Value) -> FloorDecision {
        let Some(command) = tool_input.get("command").and_then(|c| c.as_str()) else {
            return FloorDecision::Allow;
        };
        if command.trim().is_empty() {
            return FloorDecision::Allow;
        }
        match bypass::detect_command_line_bypass(command) {
            Ok(None) => FloorDecision::Allow,
            Ok(Some(detected)) => FloorDecision::Block {
                reason: format!(
                    "{detected}. The git hook stack is the shared safety contract; fix the \
                     failing gate at its cause. A harnex-generated hook names its own escape \
                     hatch, which skips that one check and never the stack; a bypass the \
                     operator truly needs is run by the operator, outside the agent."
                ),
            },
            Err(e) => FloorDecision::Skip {
                reason: format!("command not parseable: {e}"),
            },
        }
    }

    fn evaluate_write(&self, root: &Path, tool_input: &serde_json::Value) -> FloorDecision {
        let Some(file_path) = tool_input.get("file_path").and_then(|p| p.as_str()) else {
            return FloorDecision::Skip {
                reason: "no file_path in tool_input".into(),
            };
        };
        let target = grant::lexical_resolve(root, Path::new(file_path));
        // A worktree session reaches the main checkout by absolute path, and
        // the override it could grant itself there is the one this floor
        // reads. The freeze follows the same pointer the grant does, so the
        // protected set is the same set from either root.
        let canonical = grant::canonical_repo_root(root);
        let mut roots = vec![root.to_path_buf()];
        if let Some(canonical) = canonical
            && canonical != *root
        {
            roots.push(canonical);
        }
        let Some((under, rel, entry)) = roots.iter().find_map(|from| {
            let rel = target
                .strip_prefix(from)
                .ok()?
                .to_string_lossy()
                .into_owned();
            let entry = self.protected_entry(&rel)?;
            Some((from.clone(), rel, entry.to_string()))
        }) else {
            return FloorDecision::Allow;
        };
        let shown = under.join(&rel).display().to_string();
        match grant::floor_edit_grant(root) {
            FloorGrant::Granted => FloorDecision::Grant { path: shown },
            outcome => {
                let unreadable = match outcome {
                    FloorGrant::Unreadable { reason } => {
                        format!(" The override was not read: {reason}.")
                    }
                    _ => String::new(),
                };
                FloorDecision::Block {
                    reason: format!(
                        "{shown} defines the enforcement floor (protected entry: {entry}). \
                         Agent writes here are frozen so a failing gate is fixed at its cause, \
                         never by weakening what the gate verifies. For deliberate harness work \
                         the operator sets {FLOOR_EDIT_GRANT_KEY}: \"1\" in the \
                         .claude/settings.local.json env block of the main checkout — live on \
                         the next check, and removing it revokes just as immediately — or edits \
                         outside the agent, stating the reason in the commit \
                         message.{unreadable}"
                    ),
                }
            }
        }
    }

    /// The protected entry a repo-relative path falls under, or `None`.
    /// Case-insensitive — the floor also ships on case-insensitive
    /// filesystems, where `Harness.toml` lands on `harness.toml`; on a
    /// case-sensitive one the odd-cased write is a false block that surfaces
    /// and names its entry, the accepted direction. A trailing `/` marks a
    /// directory prefix; anything else is an exact repo-relative path.
    fn protected_entry(&self, rel_path: &str) -> Option<&str> {
        let lower = rel_path.to_lowercase();
        BUILT_IN_PROTECTED
            .into_iter()
            .chain(self.protected.iter().map(String::as_str))
            .find(|entry| {
                let entry_lower = entry.to_lowercase();
                if entry_lower.ends_with('/') {
                    lower.starts_with(&entry_lower)
                } else {
                    lower == entry_lower
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auditor(protected_paths: &[&str]) -> FloorAuditor {
        FloorAuditor::new(&FloorConfig {
            protected_paths: protected_paths.iter().map(|s| s.to_string()).collect(),
        })
    }

    fn bash_input(command: &str) -> serde_json::Value {
        serde_json::json!({ "command": command })
    }

    fn write_input(file_path: &str) -> serde_json::Value {
        serde_json::json!({ "file_path": file_path })
    }

    /// A main checkout whose grant is absent — the block path.
    fn plain_root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        dir
    }

    #[test]
    fn blocks_a_bash_bypass_and_allows_a_clean_command() {
        let dir = plain_root();
        let auditor = auditor(&[]);
        assert!(matches!(
            auditor.evaluate(dir.path(), "Bash", &bash_input("git commit --no-verify")),
            FloorDecision::Block { .. }
        ));
        assert_eq!(
            auditor.evaluate(dir.path(), "Bash", &bash_input("git status")),
            FloorDecision::Allow
        );
    }

    #[test]
    fn an_unparseable_command_skips_with_its_reason_rather_than_blocking() {
        let dir = plain_root();
        let decision = auditor(&[]).evaluate(dir.path(), "Bash", &bash_input("echo 'oops"));
        let FloorDecision::Skip { reason } = decision else {
            panic!("expected skip, got {decision:?}");
        };
        assert!(reason.contains("unterminated single quote"));
    }

    #[test]
    fn a_missing_or_empty_command_is_not_the_floor_s_question() {
        let dir = plain_root();
        let auditor = auditor(&[]);
        assert_eq!(
            auditor.evaluate(dir.path(), "Bash", &serde_json::json!({})),
            FloorDecision::Allow
        );
        assert_eq!(
            auditor.evaluate(dir.path(), "Bash", &bash_input("   ")),
            FloorDecision::Allow
        );
    }

    #[test]
    fn freezes_the_built_in_floor_without_any_declaration() {
        let dir = plain_root();
        let auditor = auditor(&[]);
        for path in [
            "harness.toml",
            ".claude/settings.json",
            ".claude/settings.local.json",
            "Harness.TOML",
        ] {
            assert!(
                matches!(
                    auditor.evaluate(dir.path(), "Write", &write_input(path)),
                    FloorDecision::Block { .. }
                ),
                "not frozen: {path}"
            );
        }
    }

    #[test]
    fn freezes_a_declared_entry_exactly_and_a_directory_entry_by_prefix() {
        let dir = plain_root();
        let auditor = auditor(&["hooks/", ".gitleaks.toml"]);
        assert!(matches!(
            auditor.evaluate(dir.path(), "Edit", &write_input("hooks/pre-commit")),
            FloorDecision::Block { .. }
        ));
        assert!(matches!(
            auditor.evaluate(dir.path(), "Write", &write_input(".gitleaks.toml")),
            FloorDecision::Block { .. }
        ));
        assert_eq!(
            auditor.evaluate(dir.path(), "Write", &write_input("hooks-doc.md")),
            FloorDecision::Allow
        );
        assert_eq!(
            auditor.evaluate(dir.path(), "Write", &write_input("src/main.rs")),
            FloorDecision::Allow
        );
    }

    #[test]
    fn a_dot_segment_spelling_of_a_protected_path_still_matches() {
        let dir = plain_root();
        assert!(matches!(
            auditor(&[]).evaluate(dir.path(), "Write", &write_input("./src/../harness.toml")),
            FloorDecision::Block { .. }
        ));
    }

    #[test]
    fn the_standing_grant_turns_a_block_into_a_surfaced_grant() {
        let dir = plain_root();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(
            dir.path().join(".claude/settings.local.json"),
            r#"{"env": {"HARNEX_ALLOW_FLOOR_EDIT": "1"}}"#,
        )
        .unwrap();
        let decision = auditor(&[]).evaluate(dir.path(), "Edit", &write_input("harness.toml"));
        assert!(
            matches!(decision, FloorDecision::Grant { .. }),
            "{decision:?}"
        );
    }

    #[test]
    fn an_unreadable_override_blocks_and_names_why_it_was_not_read() {
        let dir = plain_root();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(dir.path().join(".claude/settings.local.json"), "{not json").unwrap();
        let decision = auditor(&[]).evaluate(dir.path(), "Edit", &write_input("harness.toml"));
        let FloorDecision::Block { reason } = decision else {
            panic!("expected block, got {decision:?}");
        };
        assert!(reason.contains("The override was not read"));
    }

    #[test]
    fn a_worktree_write_reaching_the_main_checkout_s_floor_is_frozen() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        let worktree = dir.path().join("wt");
        let gitdir = main.join(".git/worktrees/wt");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(gitdir.join("commondir"), "../..\n").unwrap();
        std::fs::write(main.join(".git/config"), "[core]\n\tbare = false\n").unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        let target = main.join(".claude/settings.local.json");
        assert!(matches!(
            auditor(&[]).evaluate(&worktree, "Write", &write_input(&target.to_string_lossy())),
            FloorDecision::Block { .. }
        ));
    }

    #[test]
    fn a_tool_outside_the_dispatch_set_skips_as_the_matcher_s_overreach() {
        let dir = plain_root();
        let decision = auditor(&[]).evaluate(dir.path(), "Glob", &serde_json::json!({}));
        assert!(
            matches!(decision, FloorDecision::Skip { .. }),
            "{decision:?}"
        );
    }
}
