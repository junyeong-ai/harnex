//! Replacement for fragile `_runner.sh` shell wrappers.
//!
//! Resolves the project root — from `CLAUDE_PROJECT_DIR` where a runtime set
//! it, otherwise by probing git from the working directory — sets it as cwd,
//! then spawns the inner command. Returns the inner command's exit code. If
//! neither answers, returns [`HookRunOutcome::SkippedFailOpen`] with a stderr
//! advisory — exactly the discipline of the shell `_runner.sh`.
//!
//! Discovery and spawning are separate entry points, as everywhere else in
//! this crate a working directory is a parameter. Fused, the exit-code
//! contract — the whole reason the two wrappers differ — could only be
//! exercised from inside a git working tree, so it failed for anyone building
//! from a source release. [`HookRunner::run_at`] and [`HookRunner::run_stop_at`]
//! are that contract; `run` and `run_stop` are discovery over them, and the
//! fail-open branch belongs to discovery alone.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::guard::project_dir::PROJECT_DIR_ENV;

#[derive(Debug, Clone, Copy, Serialize, schemars::JsonSchema)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum HookRunOutcome {
    Completed {
        exit_code: i32,
    },
    SkippedFailOpen,
    /// Inner program ran; exit code observed but suppressed to 0 (Stop-hook contract).
    StopForcedSuccess {
        observed_exit_code: i32,
    },
}

pub struct HookRunner;

impl HookRunner {
    /// Spawn `program` with `args` from the resolved project root. Returns
    /// the inner exit code; fail-open when the project root cannot be resolved.
    pub fn run(program: &str, args: &[&str]) -> Result<HookRunOutcome> {
        match Self::resolve_root() {
            Some(root) => Self::run_at(&root, program, args),
            None => Ok(Self::fail_open()),
        }
    }

    /// The non-Stop exit-code contract, from a caller-supplied root: the inner
    /// code is propagated as [`HookRunOutcome::Completed`] so PreToolUse and
    /// PostToolUse gating works.
    pub fn run_at(root: &Path, program: &str, args: &[&str]) -> Result<HookRunOutcome> {
        Ok(HookRunOutcome::Completed {
            exit_code: Self::spawn(root, program, args)?,
        })
    }

    /// Stop / SubagentStop hook wrapper: spawn the inner program, observe
    /// its exit code, but always return exit 0 to prevent the agent from
    /// being trapped in a Stop loop. Non-zero observations are reported
    /// in the envelope payload AND emitted to stderr as an advisory.
    pub fn run_stop(program: &str, args: &[&str]) -> Result<HookRunOutcome> {
        match Self::resolve_root() {
            Some(root) => Self::run_stop_at(&root, program, args),
            None => Ok(Self::fail_open()),
        }
    }

    /// The Stop exit-code contract, from a caller-supplied root: the inner code
    /// is observed and reported as [`HookRunOutcome::StopForcedSuccess`], never
    /// propagated, because a non-zero Stop exit re-triggers the stop.
    pub fn run_stop_at(root: &Path, program: &str, args: &[&str]) -> Result<HookRunOutcome> {
        let exit_code = Self::spawn(root, program, args)?;
        if exit_code != 0 {
            eprintln!(
                "[harness-stop-advisory] inner '{program}' exited {exit_code}; \
                 Stop hook returning 0 to avoid Stop-loop trap"
            );
        }
        Ok(HookRunOutcome::StopForcedSuccess {
            observed_exit_code: exit_code,
        })
    }

    fn spawn(root: &Path, program: &str, args: &[&str]) -> Result<i32> {
        let status = Command::new(program)
            .args(args)
            .current_dir(root)
            .status()
            .map_err(|e| Error::GuardSpawnFailure {
                message: format!("spawn {program}: {e}"),
            })?;
        Ok(status.code().unwrap_or(-1))
    }

    fn fail_open() -> HookRunOutcome {
        eprintln!("[harness-skipped: project root unresolved]");
        HookRunOutcome::SkippedFailOpen
    }

    /// The runtime states the project it launched this hook for, and that
    /// statement outranks anything read from the working directory: Claude Code
    /// fires hooks from wherever the session is, so a git probe there answers
    /// about that directory instead — inside a submodule or any nested checkout
    /// it names the inner repository, and the verifier runs against the wrong
    /// tree or not at all. The probe remains for every other caller, where
    /// there is no runtime to ask.
    /// Set but unusable decides too. Falling through to the probe there would
    /// answer about the working directory — the one place the runtime does not
    /// promise anything — so a stale or mangled variable, on a hook fired from
    /// a nested checkout, would silently gate the inner repository. Skipping
    /// fails open, which is this wrapper's contract; the shell wrappers say the
    /// same by naming the root they could not enter.
    fn resolve_root() -> Option<PathBuf> {
        if let Some(dir) = std::env::var_os(PROJECT_DIR_ENV) {
            let root = PathBuf::from(dir);
            return (root.is_absolute() && root.is_dir()).then_some(root);
        }
        let cwd = std::env::current_dir().ok()?;
        Self::resolve_root_from(&cwd)
    }

    /// The git probe, anchored at `dir` rather than at the process cwd, so the
    /// two branches a hook can take are both reachable from a test.
    fn resolve_root_from(dir: &Path) -> Option<PathBuf> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(dir)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if raw.is_empty() {
            None
        } else {
            Some(PathBuf::from(raw))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The nested-checkout case the probe alone gets wrong: a hook fired from
    /// inside a submodule sits in a working tree that is not the project's, so
    /// git answers the inner repository. `run_at` takes the root as a
    /// parameter precisely so the caller's answer is the one used — this pins
    /// that the probe and the runtime disagree there, which is why
    /// `resolve_root` asks the runtime first.
    #[cfg(unix)]
    #[test]
    fn the_probe_answers_the_inner_repository_from_inside_a_nested_checkout() {
        let outer = tempfile::tempdir().expect("tempdir");
        let inner = outer.path().join("vendor/dep");
        std::fs::create_dir_all(&inner).expect("mkdir");
        for dir in [outer.path(), inner.as_path()] {
            assert!(
                Command::new("git")
                    .args(["init", "-q"])
                    .current_dir(dir)
                    .status()
                    .expect("git init")
                    .success()
            );
        }
        let probed = HookRunner::resolve_root_from(&inner).expect("probe answers");
        assert!(
            probed.ends_with("vendor/dep"),
            "the probe names the inner repository, not the project: {}",
            probed.display()
        );
    }

    /// A directory that does not exist cannot be inside a working tree, and the
    /// probe cannot even start there — the branch a hook takes on env drift,
    /// reachable without depending on where the test runner happens to sit.
    #[test]
    fn resolve_root_is_none_where_the_probe_cannot_run() {
        assert!(HookRunner::resolve_root_from(Path::new("/harnex-no-such-directory")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_root_answers_the_working_tree_it_probes_from() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(dir.path())
                .status()
                .expect("git init")
                .success()
        );
        let resolved =
            HookRunner::resolve_root_from(dir.path()).expect("initialised tree resolves");
        assert_eq!(
            std::fs::canonicalize(resolved).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }
}
