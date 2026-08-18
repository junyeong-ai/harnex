//! Wiring-integrity auditor for `.claude/settings.json` hooks.
//!
//! A hook whose script is not on disk fails open: Claude Code reports the
//! handler as errored and the action proceeds, so the harness looks wired
//! while enforcing nothing. Nothing else in the toolchain sees it — the
//! settings file parses, every event name is valid, and the formatter, the
//! linter and CI all pass. That silence is what makes a deleted verifier the
//! failure mode worth a deterministic check.
//!
//! Checks:
//! - A `command` handler naming a scaffold artifact that is absent.
//! - A scaffold artifact a handler spawns DIRECTLY that is not executable.
//!
//! ## What this module refuses to do
//!
//! - Never claim a path *should* exist without knowing what produces it. The
//!   anchor proves a token is a project path; it does not prove the project
//!   builds it. `${CLAUDE_PROJECT_DIR}/node_modules/.bin/prettier` and
//!   `${CLAUDE_PROJECT_DIR}/target/release/harness` are correct hooks that are
//!   simply absent before an install or a build, and flagging them would fail
//!   the gate on a fresh clone. The scaffold manifest is the only statement of
//!   what a harness is supposed to contain, so it is the only set this check
//!   ranges over — an artifact it declares, wired and missing, is provably a
//!   deleted verifier rather than an unbuilt one.
//!
//!   What that scoping costs, stated rather than implied: **this check
//!   protects harnex-generated wiring, not the operator's own.** A
//!   project-authored `hooks/my-guard.sh`, wired and deleted, is reported
//!   nowhere — not here, and not in coverage, which ranges over the same
//!   manifest. Catching that too needs a predicate that separates "deleted"
//!   from "not built yet" for arbitrary paths; the only decidable one is
//!   tracked-in-HEAD-but-absent-from-the-worktree, and reading HEAD means a
//!   subprocess this module refuses. Silence on a fresh clone is the better
//!   trade, but it is a trade.
//! - Never read a handler's strings with one grammar. `args` decides: present,
//!   the runtime spawns `command` directly with args as the vector and no
//!   shell, so every string is one literal path; absent, `command` is a shell
//!   string and a metacharacter ends each token. Pooling them makes a trailing
//!   flag part of a filename in one direction and truncates a spaced filename
//!   in the other.
//! - Never read a handler whose `type` is not `command`. The `http`,
//!   `mcp_tool`, `prompt`, and `agent` types carry no path this auditor can
//!   resolve, and their fields happening to hold an anchored string says
//!   nothing about a file.
//! - Never check the executable bit on a script something else runs. `bash
//!   <script>` runs a non-executable file, and the bit does not survive every
//!   checkout configuration, so a mode check there would report a working hook
//!   as broken. The executable a handler spawns ITSELF is the other case and is
//!   checked: `args` present means the runtime runs `command` with no shell, so
//!   a missing bit is EACCES before the script starts. The wrapper's own
//!   fail-open cannot cover that — it never runs — and nothing else reports it.
//! - Never interpret a runner's own dispatch convention. `args[0]` naming a
//!   verifier relative to a wrapper's directory is a contract the wrapper
//!   owns, not the spec. Whether such an artifact is present is reported by
//!   the coverage block instead, where absence is a fact rather than a defect.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::audit::AuditFindingSlug;
use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::guard::{path_in_argument, paths_in_command};

pub(crate) struct HookWiringAuditor<'a> {
    /// Every destination the scaffold manifest declares, one concrete path per
    /// language. The formatter hook is the only language-tier script a handler
    /// points at, so excluding the parameterized ones exempted the one that
    /// most needed judging.
    artifacts: &'a BTreeSet<String>,
}

impl<'a> HookWiringAuditor<'a> {
    pub(crate) fn new(artifacts: &'a BTreeSet<String>) -> Self {
        Self { artifacts }
    }

    pub(crate) fn audit_file(&self, path: &Path, project_root: &Path) -> Result<Vec<Finding>> {
        let raw = std::fs::read_to_string(path).map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        let value: Value = serde_json::from_str(&raw).map_err(|e| Error::ConfigInvalid {
            message: format!("settings.json parse: {e}"),
            location: Some(Location::file(path.to_path_buf())),
        })?;
        Ok(self.audit_value(&value, path, project_root))
    }

    pub(crate) fn audit_value(
        &self,
        value: &Value,
        path: &Path,
        project_root: &Path,
    ) -> Vec<Finding> {
        let mut findings = Vec::new();
        let Some(hooks) = value.get("hooks").and_then(|v| v.as_object()) else {
            return findings;
        };
        for (event_name, event_arr) in hooks {
            let Some(entries) = event_arr.as_array() else {
                continue;
            };
            for (entry_idx, entry) in entries.iter().enumerate() {
                let Some(handlers) = entry.get("hooks").and_then(|v| v.as_array()) else {
                    continue;
                };
                for (handler_idx, handler) in handlers.iter().enumerate() {
                    if handler.get("type").and_then(|v| v.as_str()) != Some("command") {
                        continue;
                    }
                    // `args` decides how `command` is read: present, the
                    // runtime spawns the executable directly with args as the
                    // vector and no shell, so `command` is one literal path;
                    // absent, it is a shell string a metacharacter splits.
                    let args = handler.get("args").and_then(|v| v.as_array());
                    let command = handler.get("command").and_then(|v| v.as_str());
                    let mut referenced: Vec<String> = match (command, args) {
                        (Some(c), Some(_)) => path_in_argument(c).into_iter().collect(),
                        (Some(c), None) => paths_in_command(c),
                        (None, _) => Vec::new(),
                    };
                    if let Some(args) = args {
                        referenced.extend(
                            args.iter()
                                .filter_map(|a| a.as_str())
                                .filter_map(path_in_argument),
                        );
                    }
                    for rel in referenced {
                        if !self.artifacts.contains(&rel) {
                            continue;
                        }
                        let landed = project_root.join(&rel);
                        if !landed.exists() {
                            findings.push(Finding {
                                slug: AuditFindingSlug::HookScriptMissing.as_str().into(),
                                severity: Severity::Major,
                                location: Location::file(path.to_path_buf()),
                                message: format!(
                                    "hook '{event_name}'[{entry_idx}].hooks[{handler_idx}] names the scaffold artifact '{rel}', which is not in the project — \
                                     whatever the handler wanted it for is not there"
                                ),
                                hint: Some(format!(
                                    "restore {rel}, or remove the hook entry that points at it"
                                )),
                                auto_fixable: false,
                                fix_command: None,
                            });
                            continue;
                        }
                        // Only the executable a handler spawns DIRECTLY needs
                        // the bit, and `args` is what says it does: the runtime
                        // then runs `command` itself with no shell. A verifier
                        // reached through `exec bash "$VERIFIER"` does not, and
                        // a shell-form `command` does not either.
                        if command.is_some_and(|c| path_in_argument(c).as_deref() == Some(&rel))
                            && args.is_some()
                            && !is_executable(&landed)
                        {
                            findings.push(Finding {
                                slug: AuditFindingSlug::HookNotExecutable.as_str().into(),
                                severity: Severity::Major,
                                location: Location::file(path.to_path_buf()),
                                message: format!(
                                    "hook '{event_name}'[{entry_idx}].hooks[{handler_idx}] spawns '{rel}' directly, and it is not executable — \
                                     every invocation fails before the script runs, so the wrapper's own fail-open cannot help"
                                ),
                                hint: Some(format!("chmod +x {rel}")),
                                auto_fixable: false,
                                fix_command: None,
                            });
                        }
                    }
                }
            }
        }
        findings
    }
}

/// Whether the file carries an executable bit for anyone.
///
/// Unix-only by construction: on a platform without the concept the answer is
/// yes, because a mode that does not exist cannot be the reason a hook failed.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn artifacts() -> BTreeSet<String> {
        [
            "hooks/_runner.sh",
            "hooks/post-format.sh",
            "hooks/pre-commit",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn audit(json: &str, root: &Path) -> Vec<Finding> {
        let value: Value = serde_json::from_str(json).expect("test json parses");
        let declared = artifacts();
        HookWiringAuditor::new(&declared).audit_value(
            &value,
            &PathBuf::from(".claude/settings.json"),
            root,
        )
    }

    fn command_hook(handler: serde_json::Value) -> String {
        serde_json::json!({ "hooks": { "PostToolUse": [{ "hooks": [handler] }] } }).to_string()
    }

    #[test]
    fn flags_a_declared_artifact_that_is_absent() {
        let tmp = TempDir::new().unwrap();
        let findings = audit(
            &command_hook(serde_json::json!({
                "type": "command",
                "command": "bash \"${CLAUDE_PROJECT_DIR}/hooks/_runner.sh\""
            })),
            tmp.path(),
        );
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].slug, "audit-hook-script-missing");
        assert_eq!(findings[0].severity, Severity::Major);
    }

    /// Write a hook script the way the scaffold does — `chmod 0o755`.
    fn write_runner(root: &Path, executable: bool) {
        std::fs::create_dir_all(root.join("hooks")).unwrap();
        let path = root.join("hooks/_runner.sh");
        std::fs::write(&path, "exit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if executable { 0o755 } else { 0o644 };
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let _ = executable;
    }

    fn runner_hook() -> String {
        command_hook(serde_json::json!({
            "type": "command",
            "command": "${CLAUDE_PROJECT_DIR}/hooks/_runner.sh",
            "args": ["post-format.sh"]
        }))
    }

    #[test]
    fn accepts_a_declared_artifact_that_exists() {
        let tmp = TempDir::new().unwrap();
        write_runner(tmp.path(), true);
        let findings = audit(&runner_hook(), tmp.path());
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    #[cfg(unix)]
    fn flags_a_directly_spawned_script_without_its_bit() {
        // `args` present means the runtime runs `command` itself with no shell,
        // so a missing bit is EACCES before the script starts. The wrapper's
        // own fail-open cannot cover that — it never runs — and every hook in
        // the harness dies at once with nothing reporting it.
        let tmp = TempDir::new().unwrap();
        write_runner(tmp.path(), false);
        let findings = audit(&runner_hook(), tmp.path());
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].slug, "audit-hook-not-executable");
    }

    #[test]
    #[cfg(unix)]
    fn leaves_a_shell_form_command_alone() {
        // Without `args`, `command` is a shell string: `bash <script>` runs a
        // file with no bit, so a mode check there would report a working hook
        // as broken.
        let tmp = TempDir::new().unwrap();
        write_runner(tmp.path(), false);
        let findings = audit(
            &command_hook(serde_json::json!({
                "type": "command",
                "command": "bash ${CLAUDE_PROJECT_DIR}/hooks/_runner.sh post-format.sh"
            })),
            tmp.path(),
        );
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn silent_on_a_path_the_project_builds_rather_than_ships() {
        // A correct hook naming a build or install output is absent on a fresh
        // clone. The anchor proves it is a project path; nothing proves the
        // project should already have produced it, so a gate-failing finding
        // here would red every first checkout.
        let tmp = TempDir::new().unwrap();
        for command in [
            "${CLAUDE_PROJECT_DIR}/node_modules/.bin/prettier --write",
            "${CLAUDE_PROJECT_DIR}/target/release/harness check",
            "${CLAUDE_PROJECT_DIR}/.venv/bin/python tools/fmt.py",
            "bash \"${CLAUDE_PROJECT_DIR}/dist/hooks/post-format.js\"",
        ] {
            let findings = audit(
                &command_hook(serde_json::json!({ "type": "command", "command": command })),
                tmp.path(),
            );
            assert!(
                findings.is_empty(),
                "'{command}' is not a scaffold artifact: {findings:?}"
            );
        }
    }

    #[test]
    fn ignores_a_handler_that_is_not_a_command() {
        // Only a `command` handler carries a path this auditor can resolve;
        // the other four types are read by the runtime in ways that say
        // nothing about a file.
        let tmp = TempDir::new().unwrap();
        for handler_type in ["prompt", "agent", "http", "mcp_tool"] {
            let findings = audit(
                &command_hook(serde_json::json!({
                    "type": handler_type,
                    "command": "${CLAUDE_PROJECT_DIR}/hooks/_runner.sh",
                    "args": ["${CLAUDE_PROJECT_DIR}/hooks/pre-commit"]
                })),
                tmp.path(),
            );
            assert!(
                findings.is_empty(),
                "a '{handler_type}' handler carries no resolvable path: {findings:?}"
            );
        }
    }

    #[test]
    fn a_command_carrying_flags_is_read_as_a_path_plus_flags() {
        // A correct hook wired to a present script with trailing flags must
        // stay silent; reading the whole value as one filename reported it
        // missing.
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("hooks")).unwrap();
        std::fs::write(tmp.path().join("hooks/_runner.sh"), "exit 0\n").unwrap();
        let findings = audit(
            &command_hook(serde_json::json!({
                "type": "command",
                "command": "${CLAUDE_PROJECT_DIR}/hooks/_runner.sh --verbose"
            })),
            tmp.path(),
        );
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn an_argument_keeps_a_space_the_shell_would_have_split() {
        // An `args` element is passed through literally, so the space belongs
        // to the filename — the opposite of the `command` case above.
        let tmp = TempDir::new().unwrap();
        let declared: BTreeSet<String> = ["my hooks/run.sh".to_string()].into_iter().collect();
        let value: Value = serde_json::from_str(&command_hook(serde_json::json!({
            "type": "command",
            "command": "${CLAUDE_PROJECT_DIR}/hooks/_runner.sh",
            "args": ["${CLAUDE_PROJECT_DIR}/my hooks/run.sh"]
        })))
        .unwrap();
        let findings = HookWiringAuditor::new(&declared).audit_value(
            &value,
            &PathBuf::from(".claude/settings.json"),
            tmp.path(),
        );
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].message.contains("my hooks/run.sh"));
    }

    #[test]
    fn exec_form_reads_command_as_one_literal_path() {
        // With `args` present the runtime spawns `command` directly, so a
        // space in the filename survives. Reading it with shell grammar
        // truncated the name at the space and reported the file present.
        let tmp = TempDir::new().unwrap();
        let declared: BTreeSet<String> = ["my hooks/_runner.sh".to_string()].into_iter().collect();
        let value: Value = serde_json::from_str(&command_hook(serde_json::json!({
            "type": "command",
            "command": "${CLAUDE_PROJECT_DIR}/my hooks/_runner.sh",
            "args": ["post-format.sh"]
        })))
        .unwrap();
        let findings = HookWiringAuditor::new(&declared).audit_value(
            &value,
            &PathBuf::from(".claude/settings.json"),
            tmp.path(),
        );
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].message.contains("my hooks/_runner.sh"));
    }

    #[test]
    fn reads_the_unbraced_anchor_spelling() {
        let tmp = TempDir::new().unwrap();
        let findings = audit(
            &command_hook(serde_json::json!({
                "type": "command",
                "command": "bash \"$CLAUDE_PROJECT_DIR/hooks/pre-commit\""
            })),
            tmp.path(),
        );
        assert_eq!(
            findings.len(),
            1,
            "both anchor spellings are documented: {findings:?}"
        );
    }
}
