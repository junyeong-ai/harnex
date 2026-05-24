//! Integration tests for the Claude Code runtime adapter.

use harness_core::guard::{HookRunOutcome, HookRunner};

/// Stop-class hooks MUST surface the inner exit code via the outcome enum
/// without propagating it: a non-zero inner exit produces
/// [`HookRunOutcome::StopForcedSuccess`], not [`HookRunOutcome::Completed`].
/// The CLI maps this outcome to exit 0, preventing the re-stop loop that a
/// blocking Stop hook triggers per the Claude Code spec. The test runs from
/// within the harnex repo (a git working tree), so `resolve_root` resolves.
#[cfg(unix)]
#[test]
fn run_stop_observes_nonzero_inner_without_propagating() {
    let outcome = HookRunner::run_stop("sh", &["-c", "exit 7"]).expect("spawn must succeed");
    match outcome {
        HookRunOutcome::StopForcedSuccess { observed_exit_code } => {
            assert_eq!(observed_exit_code, 7);
        }
        HookRunOutcome::SkippedFailOpen => {
            panic!("test must run inside a git working tree (cargo test from harnex/)");
        }
        HookRunOutcome::Completed { .. } => {
            panic!("run_stop must never return Completed — that would propagate exit codes");
        }
    }
}

#[cfg(unix)]
#[test]
fn run_stop_observes_zero_inner_as_stop_forced_success() {
    // Even a clean inner exit funnels through StopForcedSuccess — the outcome
    // shape itself is the Stop-safety contract, not the observed value.
    let outcome = HookRunner::run_stop("sh", &["-c", "exit 0"]).expect("spawn must succeed");
    match outcome {
        HookRunOutcome::StopForcedSuccess { observed_exit_code } => {
            assert_eq!(observed_exit_code, 0);
        }
        HookRunOutcome::SkippedFailOpen => {
            panic!("test must run inside a git working tree (cargo test from harnex/)");
        }
        HookRunOutcome::Completed { .. } => {
            panic!("run_stop must never return Completed");
        }
    }
}

/// The non-Stop wrapper has the opposite contract: it MUST propagate the
/// inner exit code as `Completed { exit_code }`, so PreToolUse / PostToolUse
/// gating works. The two wrappers' differing outcome shapes are the type-level
/// expression of Claude Code's per-event exit-code semantics.
#[cfg(unix)]
#[test]
fn run_propagates_inner_exit_code() {
    let outcome = HookRunner::run("sh", &["-c", "exit 5"]).expect("spawn must succeed");
    match outcome {
        HookRunOutcome::Completed { exit_code } => {
            assert_eq!(exit_code, 5);
        }
        HookRunOutcome::SkippedFailOpen => {
            panic!("test must run inside a git working tree (cargo test from harnex/)");
        }
        HookRunOutcome::StopForcedSuccess { .. } => {
            panic!("run must never return StopForcedSuccess — that is the Stop-wrapper shape");
        }
    }
}
