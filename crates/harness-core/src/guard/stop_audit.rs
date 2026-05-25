//! Fresh-context Stop auditor.
//!
//! Flow:
//! 1. Check `has_changes_check` — if exit 0 (no changes), allow stop.
//! 2. Bump per-session retry counter; if > max_retries, escalate via Block.
//! 3. Spawn the critique skill via `claude --print <critique_skill>` from
//!    the working directory.
//! 4. Parse critique output as JSON; if any finding has severity in
//!    {blocker}, return Block; otherwise reset counter and Allow.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::config::StopAuditConfig;
use crate::envelope::Severity;
use crate::error::{Error, Result};
use crate::path_guard;

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(tag = "decision", rename_all = "kebab-case")]
pub enum StopDecision {
    Allow,
    Block { reason: String },
}

/// Output of a spawned command, reduced to what the Stop auditor needs.
pub struct ProcessOutput {
    pub success: bool,
    pub stdout: String,
}

/// Abstracted command invocation for the Stop auditor. The trait exists for
/// **two specific reasons** (NOT speculative future flexibility):
///
/// 1. **External process boundary** — the Stop audit shells out twice (the
///    `has_changes_check` probe and the `claude --print` critique). Wrapping
///    that boundary in a trait keeps `std::process::Command` out of the audit
///    flow and confines it to one impl.
/// 2. **Test seam** — `StopAuditor::with_runner` substitutes a mock that
///    returns canned [`ProcessOutput`] responses, so the 3-phase decision flow
///    is verified without spawning `git` or `claude` in CI.
///
/// New spawn sites in the Stop audit should call `self.runner.run(...)`, not
/// reach for `Command` directly. Adding a second production runner impl beyond
/// [`DefaultCommandRunner`] + the test mock is YAGNI — push back on it.
pub trait CommandRunner: Send + Sync {
    /// Run `program` with `args` in `cwd`. Returns exit-success + captured stdout.
    fn run(&self, program: &str, args: &[&str], cwd: &Path) -> Result<ProcessOutput>;
}

/// Spawns a real binary, capturing exit status and stdout.
pub struct DefaultCommandRunner;

impl CommandRunner for DefaultCommandRunner {
    fn run(&self, program: &str, args: &[&str], cwd: &Path) -> Result<ProcessOutput> {
        let output = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .output()
            .map_err(|e| Error::GuardSpawnFailure {
                message: format!("spawn '{program}': {e}"),
            })?;
        Ok(ProcessOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        })
    }
}

pub struct StopAuditor<'a, R: CommandRunner = DefaultCommandRunner> {
    config: &'a StopAuditConfig,
    working_dir: &'a Path,
    session_id: String,
    runner: R,
}

impl<'a> StopAuditor<'a, DefaultCommandRunner> {
    /// Construct with the real process runner. `new` (not `with_runner`)
    /// carries the production constructor so the CLI stays runner-agnostic;
    /// tests inject a mock via [`StopAuditor::with_runner`].
    pub fn new(config: &'a StopAuditConfig, working_dir: &'a Path, session_id: String) -> Self {
        Self::with_runner(config, working_dir, session_id, DefaultCommandRunner)
    }
}

impl<'a, R: CommandRunner> StopAuditor<'a, R> {
    pub fn with_runner(
        config: &'a StopAuditConfig,
        working_dir: &'a Path,
        session_id: String,
        runner: R,
    ) -> Self {
        Self {
            config,
            working_dir,
            session_id: safe_session_id(&session_id),
            runner,
        }
    }

    pub fn run(&self) -> Result<StopDecision> {
        if !self.has_changes()? {
            return Ok(StopDecision::Allow);
        }
        let attempt = self.bump_retry_counter()?;
        if attempt > self.config.max_retries {
            return Ok(StopDecision::Block {
                reason: format!(
                    "retry counter exceeded {} — escalating to user",
                    self.config.max_retries
                ),
            });
        }
        let critique_output = self.spawn_critique()?;
        if has_gating_finding(&critique_output) {
            Ok(StopDecision::Block {
                reason: format!(
                    "critique skill '{}' returned blocker-severity findings",
                    self.config.critique_skill
                ),
            })
        } else {
            self.clear_retry_counter()?;
            Ok(StopDecision::Allow)
        }
    }

    fn has_changes(&self) -> Result<bool> {
        if self.config.has_changes_check.is_empty() {
            return Ok(true);
        }
        let (program, args) = self.config.has_changes_check.split_first().unwrap();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = self.runner.run(program, &args, self.working_dir)?;
        // Convention: exit 0 == no changes; non-zero == changes present.
        Ok(!output.success)
    }

    fn retry_path(&self) -> PathBuf {
        self.working_dir
            .join(&self.config.retry_ledger_dir)
            .join(format!("{}.count", self.session_id))
    }

    fn bump_retry_counter(&self) -> Result<u32> {
        let path = self.retry_path();
        let current = match std::fs::read_to_string(&path) {
            // No ledger yet: this is the first stop of the session.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            // Ledger present but unreadable (IO error): fail safe by assuming
            // the bound is reached, so the next bump escalates rather than
            // silently resetting the loop guard to zero.
            Err(_) => self.config.max_retries,
            // Ledger present but corrupt (not a u32): same fail-safe.
            Ok(s) => s.trim().parse::<u32>().unwrap_or(self.config.max_retries),
        };
        let next = current.saturating_add(1);
        path_guard::write_atomic(&path, next.to_string().as_bytes())?;
        Ok(next)
    }

    fn clear_retry_counter(&self) -> Result<()> {
        let path = self.retry_path();
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| Error::IoFailure {
                path: path.clone(),
                source: e,
            })?;
        }
        Ok(())
    }

    fn spawn_critique(&self) -> Result<String> {
        let output = self.runner.run(
            "claude",
            &["--print", &self.config.critique_skill],
            self.working_dir,
        )?;
        Ok(output.stdout)
    }
}

fn safe_session_id(raw: &str) -> String {
    if !raw.is_empty()
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        raw.to_string()
    } else {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        raw.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

/// Inspect a critique envelope for any finding that fails the gate
/// ([`Severity::fails_gate`] — `blocker` or `major`), the same threshold the
/// CLI gate uses. Returns false on parse failure (fail-OPEN by design:
/// Constitution V — the session never traps; a broken critique tool must not
/// imprison the agent in a re-stop loop. The bounded retry counter, not a
/// fail-closed gate, is the loop's safety net).
fn has_gating_finding(critique_output: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(critique_output) else {
        return false;
    };
    let Some(items) = value
        .get("data")
        .and_then(|d| d.get("items"))
        .and_then(|i| i.as_array())
    else {
        return false;
    };
    items.iter().any(|item| {
        item.get("severity")
            .and_then(|s| serde_json::from_value::<Severity>(s.clone()).ok())
            .is_some_and(Severity::fails_gate)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Queues [`ProcessOutput`] responses popped in call order: the Stop audit
    /// spawns `has_changes_check` first, then the critique skill — so a queue
    /// of two responses maps positionally to those two phases.
    struct MockCommandRunner {
        responses: Mutex<Vec<ProcessOutput>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl MockCommandRunner {
        fn new(responses: Vec<ProcessOutput>) -> Self {
            Self {
                responses: Mutex::new(responses),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn out(success: bool, stdout: &str) -> ProcessOutput {
            ProcessOutput {
                success,
                stdout: stdout.to_string(),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl CommandRunner for MockCommandRunner {
        fn run(&self, program: &str, args: &[&str], _cwd: &Path) -> Result<ProcessOutput> {
            let mut record = vec![program.to_string()];
            record.extend(args.iter().map(|s| s.to_string()));
            self.calls.lock().unwrap().push(record);
            let mut resp = self.responses.lock().unwrap();
            // Exhaustion is a test bug: an unexpected extra spawn must fail
            // loudly rather than be masked by a default success response.
            assert!(
                !resp.is_empty(),
                "MockCommandRunner exhausted: unexpected spawn of '{program}'"
            );
            Ok(resp.remove(0))
        }
    }

    fn audit_config(dir: &TempDir) -> StopAuditConfig {
        StopAuditConfig {
            runtime: "claude-code".to_string(),
            critique_skill: "/aix-critique".to_string(),
            max_retries: 3,
            has_changes_check: vec!["git".into(), "diff".into(), "--quiet".into()],
            retry_ledger_dir: dir.path().join("_audit_retry"),
        }
    }

    const CLEAN_ENVELOPE: &str = r#"{"ok":true,"data":{"items":[],"total":0}}"#;
    const BLOCKER_ENVELOPE: &str = r#"{"ok":true,"data":{"items":[
        {"slug":"x","severity":"blocker","location":{"path":"a"},"message":"oops"}
    ],"total":1}}"#;

    #[test]
    fn run_allows_when_no_changes() {
        let dir = TempDir::new().unwrap();
        let config = audit_config(&dir);
        // exit 0 from has_changes_check == no changes.
        let runner = MockCommandRunner::new(vec![MockCommandRunner::out(true, "")]);
        let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
        let decision = auditor.run().unwrap();
        assert!(matches!(decision, StopDecision::Allow));
        // Only the has_changes probe ran; the critique was never spawned.
        assert_eq!(auditor.runner.call_count(), 1);
    }

    #[test]
    fn run_blocks_on_blocker_critique() {
        let dir = TempDir::new().unwrap();
        let config = audit_config(&dir);
        let runner = MockCommandRunner::new(vec![
            MockCommandRunner::out(false, ""), // changes present
            MockCommandRunner::out(true, BLOCKER_ENVELOPE),
        ]);
        let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
        let decision = auditor.run().unwrap();
        assert!(matches!(decision, StopDecision::Block { .. }));
        // Verify the seam dispatched the exact configured commands in order:
        // the has_changes probe first, then the critique skill spawn.
        let calls = auditor.runner.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], vec!["git", "diff", "--quiet"]);
        assert_eq!(calls[1], vec!["claude", "--print", "/aix-critique"]);
    }

    #[test]
    fn run_allows_and_clears_on_clean_critique() {
        let dir = TempDir::new().unwrap();
        let config = audit_config(&dir);
        let runner = MockCommandRunner::new(vec![
            MockCommandRunner::out(false, ""), // changes present
            MockCommandRunner::out(true, CLEAN_ENVELOPE),
        ]);
        let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
        let decision = auditor.run().unwrap();
        assert!(matches!(decision, StopDecision::Allow));
        // A clean critique resets the retry ledger.
        assert!(!auditor.retry_path().exists());
    }

    #[test]
    fn run_blocks_when_retry_exceeds_max() {
        let dir = TempDir::new().unwrap();
        let config = audit_config(&dir);
        // Only the has_changes probe should run: escalation happens before the
        // critique spawn, so a single "changes present" response is enough.
        let runner = MockCommandRunner::new(vec![MockCommandRunner::out(false, "")]);
        let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
        // Pre-seed the ledger at max_retries; the next bump (max + 1) exceeds it.
        path_guard::write_atomic(
            &auditor.retry_path(),
            config.max_retries.to_string().as_bytes(),
        )
        .unwrap();
        let decision = auditor.run().unwrap();
        match decision {
            StopDecision::Block { reason } => {
                assert!(reason.contains("retry counter exceeded"), "got: {reason}");
            }
            StopDecision::Allow => panic!("expected escalation Block"),
        }
        // The critique was never spawned — only the has_changes probe ran.
        assert_eq!(auditor.runner.call_count(), 1);
    }

    #[test]
    fn run_escalates_on_corrupt_retry_ledger_instead_of_resetting() {
        let dir = TempDir::new().unwrap();
        let config = audit_config(&dir);
        let runner = MockCommandRunner::new(vec![MockCommandRunner::out(false, "")]);
        let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
        // A corrupt (non-numeric) ledger must NOT reset the loop guard to 0;
        // it fails safe to max_retries so the next bump escalates.
        path_guard::write_atomic(&auditor.retry_path(), b"not-a-number").unwrap();
        match auditor.run().unwrap() {
            StopDecision::Block { reason } => {
                assert!(reason.contains("retry counter exceeded"), "got: {reason}");
            }
            StopDecision::Allow => panic!("corrupt ledger must fail safe, not reset"),
        }
        assert_eq!(auditor.runner.call_count(), 1);
    }

    #[test]
    fn gating_finding_detects_blocker() {
        let json = r#"{"ok":true,"data":{"items":[
            {"slug":"x","severity":"blocker","location":{"path":"a"},"message":"oops"}
        ],"total":1}}"#;
        assert!(has_gating_finding(json));
    }

    #[test]
    fn gating_finding_detects_major() {
        // Major also fails the gate (Severity::fails_gate) — a Major critique
        // finding must block the stop, matching the CLI gate threshold.
        let json = r#"{"ok":true,"data":{"items":[
            {"slug":"x","severity":"major","location":{"path":"a"},"message":"defect"}
        ],"total":1}}"#;
        assert!(has_gating_finding(json));
    }

    #[test]
    fn gating_finding_ignores_minor_findings() {
        let json = r#"{"ok":true,"data":{"items":[
            {"slug":"x","severity":"minor","location":{"path":"a"},"message":"meh"}
        ],"total":1}}"#;
        assert!(!has_gating_finding(json));
    }

    #[test]
    fn gating_finding_handles_empty_findings() {
        let json = r#"{"ok":true,"data":{"items":[],"total":0}}"#;
        assert!(!has_gating_finding(json));
    }

    #[test]
    fn gating_finding_handles_parse_failure() {
        // Fail-open: a malformed critique must not trap the session (Const. V).
        assert!(!has_gating_finding("not json"));
    }

    #[test]
    fn safe_session_id_passes_valid_ids() {
        assert_eq!(safe_session_id("abc-123_XYZ"), "abc-123_XYZ");
        assert_eq!(safe_session_id("simple"), "simple");
    }

    #[test]
    fn safe_session_id_sanitizes_path_separators() {
        let sanitized = safe_session_id("../../etc/passwd");
        assert!(
            sanitized.chars().all(|c| c.is_ascii_hexdigit()),
            "expected hex hash, got: {sanitized}"
        );
        assert_eq!(sanitized.len(), 16);
    }

    #[test]
    fn safe_session_id_sanitizes_empty() {
        let sanitized = safe_session_id("");
        assert!(
            sanitized.chars().all(|c| c.is_ascii_hexdigit()),
            "expected hex hash, got: {sanitized}"
        );
        assert_eq!(sanitized.len(), 16);
    }
}
