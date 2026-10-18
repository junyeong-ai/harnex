//! Fresh-context Stop auditor.
//!
//! Flow:
//! 1. Run the configured changes probe — exit 0 (no changes) allows the stop,
//!    and anything but its two answers is a [`StopDecision::Skip`].
//! 2. Reserve a critique attempt; a spent budget escalates to the operator
//!    without requesting continuation.
//! 3. Spawn the critique skill via `claude --print <critique_skill>` from
//!    the working directory.
//! 4. A valid critique blocks on gating findings and otherwise allows the
//!    stop. A failed or malformed critique is a visible skip.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::config::StopAuditConfig;
use crate::envelope::Severity;
use crate::error::{Error, Result};
use crate::path_guard;

/// One stop, judged. The CLI matches on this to speak the Stop hook contract
/// directly, so it is never serialised — no wire derives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopDecision {
    Allow,
    /// The audit could not reach a verdict, and says why. Never a Block: on
    /// the Stop event a block forces continuation, so an auditor that cannot
    /// run would hold the session open at every Stop with nothing the operator
    /// can act on and no retry bound, which is the loop the counter exists to
    /// prevent arriving through the one door it does not watch.
    Skip {
        reason: String,
    },
    /// End automatic continuation and hand the unapproved work to the operator.
    Escalate {
        reason: String,
    },
    Block {
        reason: String,
    },
}

/// Output of a spawned command, reduced to what the Stop auditor needs.
pub struct ProcessOutput {
    /// The process's exit code, or `None` where a signal ended it.
    pub code: Option<i32>,
    pub stdout: String,
}

impl ProcessOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// What the configured probe said about work this session left behind.
enum Changes {
    /// Exit 0 — nothing to critique, so stopping costs nothing.
    None,
    /// Exit 1 — the predicate is false, so there is work.
    Present,
    /// The probe did not answer: it could not run, or it exited outside the
    /// convention it is written against. Never read as `Present`, which would
    /// spend a model call on a broken probe at every Stop.
    Unknown(String),
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
            code: output.status.code(),
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
        match self.changes() {
            Changes::None => return Ok(StopDecision::Allow),
            Changes::Unknown(reason) => return Ok(StopDecision::Skip { reason }),
            Changes::Present => {}
        }
        let attempt = self.bump_retry_counter()?;
        if attempt > self.config.max_retries {
            return Ok(StopDecision::Escalate {
                reason: format!(
                    "retry budget of {} exhausted — automatic audit stopped; operator review is required",
                    self.config.max_retries
                ),
            });
        }
        let output = self.spawn_critique()?;
        if !output.success() {
            let reason = match output.code {
                Some(code) => format!("critique exited {code} without a successful result"),
                None => "critique was killed without a result".into(),
            };
            return Ok(StopDecision::Skip { reason });
        }
        let gating = match has_gating_finding(&output.stdout) {
            Ok(gating) => gating,
            Err(reason) => return Ok(StopDecision::Skip { reason }),
        };
        if gating {
            Ok(StopDecision::Block {
                reason: format!(
                    "critique skill '{}' returned gating findings",
                    self.config.critique_skill
                ),
            })
        } else {
            self.clear_retry_counter()?;
            Ok(StopDecision::Allow)
        }
    }

    /// The probe is a shell predicate, read the way the ecosystem writes one:
    /// 0 is true, 1 is false, anything else is the predicate failing rather
    /// than answering. Measured, `git diff --quiet` exits 0 and 1 for its two
    /// answers and 129 on a flag it does not know, so folding every non-zero
    /// code into "there is work" buys a critique on a probe that never ran.
    fn changes(&self) -> Changes {
        let (program, args) = match self.config.changes_probe() {
            Ok(probe) => probe,
            Err(e) => return Changes::Unknown(e.to_string()),
        };
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.runner.run(program, &args, self.working_dir) {
            Err(e) => Changes::Unknown(format!("changes probe did not run: {e}")),
            Ok(output) => match output.code {
                Some(0) => Changes::None,
                Some(1) => Changes::Present,
                Some(code) => Changes::Unknown(format!(
                    "changes probe '{program}' exited {code}, which is neither of the \
                     answers it is asked for (0 clean, 1 work left behind)"
                )),
                None => Changes::Unknown(format!("changes probe '{program}' was killed")),
            },
        }
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

    fn spawn_critique(&self) -> Result<ProcessOutput> {
        self.runner.run(
            "claude",
            &["--print", &self.config.critique_skill],
            self.working_dir,
        )
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

#[derive(Deserialize)]
struct CritiqueEnvelope {
    ok: bool,
    data: CritiqueData,
}

#[derive(Deserialize)]
struct CritiqueData {
    items: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct CritiqueFinding {
    severity: Severity,
}

fn has_gating_finding(critique_output: &str) -> std::result::Result<bool, String> {
    let critique: CritiqueEnvelope = serde_json::from_str(critique_output)
        .map_err(|e| format!("critique returned an invalid finding envelope: {e}"))?;
    if !critique.ok {
        return Err("critique reported an unsuccessful result".into());
    }
    let mut unreadable = None;
    for item in critique.data.items {
        match serde_json::from_value::<CritiqueFinding>(item) {
            Ok(item) if item.severity.fails_gate() => return Ok(true),
            Ok(_) => {}
            Err(error) => {
                unreadable.get_or_insert(error);
            }
        }
    }
    match unreadable {
        Some(error) => Err(format!("critique returned an unreadable finding: {error}")),
        None => Ok(false),
    }
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

        /// A canned response, named by the exit code the probe or the
        /// critique returns — the auditor reads 0 and 1 as answers and
        /// anything else as the command failing to give one.
        fn exits(code: i32, stdout: &str) -> ProcessOutput {
            ProcessOutput {
                code: Some(code),
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
            has_changes_check: vec![
                "sh".into(),
                "-c".into(),
                "test -z \"$(git status --porcelain)\"".into(),
            ],
            retry_ledger_dir: dir.path().join("_audit_retry"),
        }
    }

    const CLEAN_ENVELOPE: &str = r#"{"ok":true,"data":{"items":[],"total":0}}"#;
    const BLOCKER_ENVELOPE: &str = r#"{"ok":true,"data":{"items":[
        {"slug":"x","severity":"blocker","location":{"path":"a"},"message":"oops"}
    ],"total":1}}"#;

    #[test]
    fn a_spent_budget_stops_requesting_continuation_without_another_critique() {
        let dir = TempDir::new().unwrap();
        let mut config = audit_config(&dir);
        config.max_retries = 1;
        let runner = MockCommandRunner::new(vec![
            MockCommandRunner::exits(1, ""),
            MockCommandRunner::exits(0, BLOCKER_ENVELOPE),
            MockCommandRunner::exits(1, ""),
            MockCommandRunner::exits(1, ""),
            MockCommandRunner::exits(1, ""),
        ]);
        let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
        assert!(matches!(auditor.run().unwrap(), StopDecision::Block { .. }));
        for _ in 0..3 {
            assert!(matches!(
                auditor.run().unwrap(),
                StopDecision::Escalate { .. }
            ));
        }
        assert_eq!(auditor.runner.call_count(), 5);
    }

    #[test]
    fn an_unanswered_critique_skips_without_clearing_the_budget() {
        for (code, stdout) in [
            (Some(1), CLEAN_ENVELOPE),
            (None, CLEAN_ENVELOPE),
            (Some(0), "not json"),
            (Some(0), r#"{"ok":false,"data":{"items":[]}}"#),
            (Some(0), r#"{"ok":true,"data":{}}"#),
            (Some(0), r#"{"ok":true,"data":{"items":[{}]}}"#),
            (
                Some(0),
                r#"{"ok":true,"data":{"items":[{"severity":"unknown"}]}}"#,
            ),
        ] {
            let dir = TempDir::new().unwrap();
            let config = audit_config(&dir);
            let runner = MockCommandRunner::new(vec![
                MockCommandRunner::exits(1, ""),
                ProcessOutput {
                    code,
                    stdout: stdout.into(),
                },
            ]);
            let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
            assert!(
                matches!(auditor.run().unwrap(), StopDecision::Skip { .. }),
                "{code:?}: {stdout}"
            );
            assert_eq!(std::fs::read_to_string(auditor.retry_path()).unwrap(), "1");
        }
    }

    #[test]
    fn a_probe_that_did_not_answer_skips_rather_than_buying_a_critique() {
        // Every way the probe can fail to answer, and none of them may reach
        // the critique: a Block would force continuation at every Stop, and
        // reading the failure as "there is work" would spend a model call on
        // a command that never ran.
        let dir = TempDir::new().unwrap();
        for (code, what) in [(2, "outside the convention"), (129, "a flag it rejects")] {
            let config = audit_config(&dir);
            let runner = MockCommandRunner::new(vec![MockCommandRunner::exits(code, "")]);
            let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
            match auditor.run().unwrap() {
                StopDecision::Skip { reason } => {
                    assert!(reason.contains(&code.to_string()), "{what}: {reason}");
                }
                other => panic!("exit {code} ({what}) must skip, got {other:?}"),
            }
            assert_eq!(auditor.runner.call_count(), 1, "the critique never ran");
        }

        // And a section that never said when it fires, which the loader
        // refuses but a hand-built config can still hold.
        let mut unstated = audit_config(&dir);
        unstated.has_changes_check.clear();
        let runner = MockCommandRunner::new(vec![]);
        let auditor = StopAuditor::with_runner(&unstated, dir.path(), "sess".into(), runner);
        assert!(matches!(auditor.run().unwrap(), StopDecision::Skip { .. }));
        assert_eq!(auditor.runner.call_count(), 0, "nothing was spawned");
    }

    #[test]
    fn run_allows_when_no_changes() {
        let dir = TempDir::new().unwrap();
        let config = audit_config(&dir);
        // exit 0 from has_changes_check == no changes.
        let runner = MockCommandRunner::new(vec![MockCommandRunner::exits(0, "")]);
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
            MockCommandRunner::exits(1, ""), // changes present
            MockCommandRunner::exits(0, BLOCKER_ENVELOPE),
        ]);
        let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
        let decision = auditor.run().unwrap();
        assert!(matches!(decision, StopDecision::Block { .. }));
        // Verify the seam dispatched the exact configured commands in order:
        // the has_changes probe first, then the critique skill spawn.
        let calls = auditor.runner.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0],
            vec!["sh", "-c", "test -z \"$(git status --porcelain)\""]
        );
        assert_eq!(calls[1], vec!["claude", "--print", "/aix-critique"]);
    }

    #[test]
    fn run_allows_and_clears_on_clean_critique() {
        let dir = TempDir::new().unwrap();
        let config = audit_config(&dir);
        let runner = MockCommandRunner::new(vec![
            MockCommandRunner::exits(1, ""), // changes present
            MockCommandRunner::exits(0, CLEAN_ENVELOPE),
        ]);
        let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
        let decision = auditor.run().unwrap();
        assert!(matches!(decision, StopDecision::Allow));
        // A clean critique resets the retry ledger.
        assert!(!auditor.retry_path().exists());
    }

    #[test]
    fn run_escalates_when_retry_exceeds_max() {
        let dir = TempDir::new().unwrap();
        let config = audit_config(&dir);
        // Only the has_changes probe should run: escalation happens before the
        // critique spawn, so a single "changes present" response is enough.
        let runner = MockCommandRunner::new(vec![MockCommandRunner::exits(1, "")]);
        let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
        // Pre-seed the ledger at max_retries; the next bump (max + 1) exceeds it.
        path_guard::write_atomic(
            &auditor.retry_path(),
            config.max_retries.to_string().as_bytes(),
        )
        .unwrap();
        let decision = auditor.run().unwrap();
        match decision {
            StopDecision::Escalate { reason } => {
                assert!(reason.contains("retry budget"), "got: {reason}");
            }
            other => panic!("expected escalation, got {other:?}"),
        }
        // The critique was never spawned — only the has_changes probe ran.
        assert_eq!(auditor.runner.call_count(), 1);
    }

    #[test]
    fn run_escalates_on_corrupt_retry_ledger_instead_of_resetting() {
        let dir = TempDir::new().unwrap();
        let config = audit_config(&dir);
        let runner = MockCommandRunner::new(vec![MockCommandRunner::exits(1, "")]);
        let auditor = StopAuditor::with_runner(&config, dir.path(), "sess".into(), runner);
        // A corrupt (non-numeric) ledger must NOT reset the loop guard to 0;
        // it fails safe to max_retries so the next bump escalates.
        path_guard::write_atomic(&auditor.retry_path(), b"not-a-number").unwrap();
        match auditor.run().unwrap() {
            StopDecision::Escalate { reason } => {
                assert!(reason.contains("retry budget"), "got: {reason}");
            }
            other => panic!("corrupt ledger must fail safe, not reset: {other:?}"),
        }
        assert_eq!(auditor.runner.call_count(), 1);
    }

    #[test]
    fn gating_finding_detects_blocker() {
        let json = r#"{"ok":true,"data":{"items":[
            {"slug":"x","severity":"blocker","location":{"path":"a"},"message":"oops"}
        ],"total":1}}"#;
        assert!(has_gating_finding(json).unwrap());
    }

    #[test]
    fn gating_finding_detects_major() {
        // Major also fails the gate (Severity::fails_gate) — a Major critique
        // finding must block the stop, matching the CLI gate threshold.
        let json = r#"{"ok":true,"data":{"items":[
            {"slug":"x","severity":"major","location":{"path":"a"},"message":"defect"}
        ],"total":1}}"#;
        assert!(has_gating_finding(json).unwrap());
    }

    #[test]
    fn gating_finding_ignores_minor_findings() {
        let json = r#"{"ok":true,"data":{"items":[
            {"slug":"x","severity":"minor","location":{"path":"a"},"message":"meh"}
        ],"total":1}}"#;
        assert!(!has_gating_finding(json).unwrap());
    }

    #[test]
    fn gating_finding_handles_empty_findings() {
        let json = r#"{"ok":true,"data":{"items":[],"total":0}}"#;
        assert!(!has_gating_finding(json).unwrap());
    }

    #[test]
    fn gating_finding_handles_parse_failure() {
        assert!(has_gating_finding("not json").is_err());
    }

    #[test]
    fn gating_finding_survives_an_unreadable_neighbor_in_either_order() {
        for unreadable in [
            serde_json::json!({}),
            serde_json::json!({"severity": "critical"}),
            serde_json::Value::Null,
        ] {
            for items in [
                serde_json::json!([unreadable, {"severity": "blocker"}]),
                serde_json::json!([{"severity": "major"}, unreadable]),
            ] {
                let output = serde_json::json!({"ok": true, "data": {"items": items}}).to_string();
                assert!(has_gating_finding(&output).unwrap(), "{output}");
            }
        }
        let output = r#"{"ok":true,"data":{"items":[{"severity":"minor"},{}]}}"#;
        assert!(has_gating_finding(output).is_err());
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
