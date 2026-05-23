//! Replacement for fragile `_runner.sh` shell wrappers.
//!
//! Resolves the project root via `git rev-parse --show-toplevel`, sets it
//! as cwd, then spawns the inner command. Returns the inner command's
//! exit code. If git probe fails, returns [`HookRunOutcome::SkippedFailOpen`]
//! with a stderr advisory — exactly the discipline of the shell `_runner.sh`.

use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, Serialize, schemars::JsonSchema)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum HookRunOutcome {
    Completed { exit_code: i32 },
    SkippedFailOpen,
    /// Inner program ran; exit code observed but suppressed to 0 (Stop-hook contract).
    StopForcedSuccess { observed_exit_code: i32 },
}

pub struct HookRunner;

impl HookRunner {
    /// Spawn `program` with `args` from the resolved project root. Returns
    /// the inner exit code; fail-open when the project root cannot be resolved.
    pub fn run(program: &str, args: &[&str]) -> Result<HookRunOutcome> {
        let root = match Self::resolve_root() {
            Some(p) => p,
            None => {
                eprintln!("[harness-skipped: project root unresolved]");
                return Ok(HookRunOutcome::SkippedFailOpen);
            }
        };
        let status = Command::new(program)
            .args(args)
            .current_dir(&root)
            .status()
            .map_err(|e| Error::GuardSpawnFailure {
                message: format!("spawn {program}: {e}"),
            })?;
        let exit_code = status.code().unwrap_or(-1);
        Ok(HookRunOutcome::Completed { exit_code })
    }

    /// Stop / SubagentStop hook wrapper: spawn the inner program, observe
    /// its exit code, but always return exit 0 to prevent the agent from
    /// being trapped in a Stop loop. Non-zero observations are reported
    /// in the envelope payload AND emitted to stderr as an advisory.
    pub fn run_stop(program: &str, args: &[&str]) -> Result<HookRunOutcome> {
        let root = match Self::resolve_root() {
            Some(p) => p,
            None => {
                eprintln!("[harness-skipped: project root unresolved]");
                return Ok(HookRunOutcome::SkippedFailOpen);
            }
        };
        let status = Command::new(program)
            .args(args)
            .current_dir(&root)
            .status()
            .map_err(|e| Error::GuardSpawnFailure {
                message: format!("spawn {program}: {e}"),
            })?;
        let exit_code = status.code().unwrap_or(-1);
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

    fn resolve_root() -> Option<PathBuf> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
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
