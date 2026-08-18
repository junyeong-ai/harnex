//! Integration tests for the Claude Code runtime adapter.

use harness_core::guard::{HookRunOutcome, HookRunner};

/// The two wrappers differ only in what they do with the inner exit code, so
/// that is what these pin — from an explicit root, because the contract is
/// about the code and not about where the root came from. Fused to discovery
/// they asserted a git working tree, which a source release does not carry.
fn root() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// Stop-class hooks MUST surface the inner exit code via the outcome enum
/// without propagating it: a non-zero inner exit produces
/// [`HookRunOutcome::StopForcedSuccess`], not [`HookRunOutcome::Completed`].
/// The CLI maps this outcome to exit 0, preventing the re-stop loop that a
/// blocking Stop hook triggers per the Claude Code spec.
#[cfg(unix)]
#[test]
fn run_stop_observes_nonzero_inner_without_propagating() {
    let dir = root();
    let outcome =
        HookRunner::run_stop_at(dir.path(), "sh", &["-c", "exit 7"]).expect("spawn must succeed");
    match outcome {
        HookRunOutcome::StopForcedSuccess { observed_exit_code } => {
            assert_eq!(observed_exit_code, 7);
        }
        other => {
            panic!("run_stop_at must never return {other:?} — that would propagate exit codes")
        }
    }
}

#[cfg(unix)]
#[test]
fn run_stop_observes_zero_inner_as_stop_forced_success() {
    // Even a clean inner exit funnels through StopForcedSuccess — the outcome
    // shape itself is the Stop-safety contract, not the observed value.
    let dir = root();
    let outcome =
        HookRunner::run_stop_at(dir.path(), "sh", &["-c", "exit 0"]).expect("spawn must succeed");
    match outcome {
        HookRunOutcome::StopForcedSuccess { observed_exit_code } => {
            assert_eq!(observed_exit_code, 0);
        }
        other => panic!("run_stop_at must never return {other:?}"),
    }
}

/// The non-Stop wrapper has the opposite contract: it MUST propagate the
/// inner exit code as `Completed { exit_code }`, so PreToolUse / PostToolUse
/// gating works. The two wrappers' differing outcome shapes are the type-level
/// expression of Claude Code's per-event exit-code semantics.
#[cfg(unix)]
#[test]
fn run_propagates_inner_exit_code() {
    let dir = root();
    let outcome =
        HookRunner::run_at(dir.path(), "sh", &["-c", "exit 5"]).expect("spawn must succeed");
    match outcome {
        HookRunOutcome::Completed { exit_code } => {
            assert_eq!(exit_code, 5);
        }
        other => panic!("run_at must never return {other:?} — that is the Stop-wrapper shape"),
    }
}

/// The wrapper runs the inner command FROM the root it was given, which is the
/// whole point of resolving one: a verifier invoked by relative path, or one
/// that reads a config beside it, is looking at the project and not at whatever
/// directory Claude Code happened to spawn the hook from.
#[cfg(unix)]
#[test]
fn the_inner_command_runs_from_the_given_root() {
    let dir = root();
    std::fs::write(dir.path().join("sentinel"), "").unwrap();
    let outcome = HookRunner::run_at(dir.path(), "sh", &["-c", "test -f sentinel"])
        .expect("spawn must succeed");
    assert!(matches!(
        outcome,
        HookRunOutcome::Completed { exit_code: 0 }
    ));
}

/// A program that is not on PATH is a typed spawn failure, never a silent
/// success — a hook that cannot start must not read as a hook that passed.
#[test]
fn a_missing_program_is_a_typed_failure() {
    let dir = root();
    let err = HookRunner::run_at(dir.path(), "harnex-no-such-program", &[]).unwrap_err();
    assert_eq!(err.code().as_str(), "GUARD_SPAWN_FAILURE");
}
