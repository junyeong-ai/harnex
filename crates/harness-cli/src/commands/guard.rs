use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use harness_core::error::{Error, Result};
use harness_core::guard::{HookEvent, HookRunOutcome, HookRunner, StopAuditor, StopDecision};

use super::{config_dir, load_config, write_envelope_success};

#[derive(Subcommand)]
pub enum GuardCommand {
    /// Parse a hook stdin JSON, echo the typed view (one-shot validator)
    HookEvent,
    /// Run `program` with `args` from the resolved project root, fail-open on env drift
    HookRun {
        /// Inner program to spawn
        program: String,
        /// Arguments forwarded to the inner program
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Stop / SubagentStop hook wrapper — spawn `program`, observe its
    /// exit code, but always return exit 0 to prevent the agent from being
    /// trapped in a Stop loop (non-zero exit triggers re-stop). The
    /// observed exit code is captured in the envelope for telemetry.
    HookStop {
        /// Inner program to spawn
        program: String,
        /// Arguments forwarded to the inner program
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run the fresh-context Stop audit. Reads session_id from --session
    /// or from hook stdin JSON's `session_id` field if not given. Wire as a
    /// Stop hook directly (NOT through `_stop_runner.sh`): Block exits 2 to
    /// force continuation, which the bounded retry counter prevents from
    /// looping. Allow exits 0.
    StopAudit {
        #[arg(long)]
        session: Option<String>,
    },
}

pub fn run<W: Write>(cmd: GuardCommand, out: &mut W) -> Result<ExitCode> {
    match cmd {
        GuardCommand::HookEvent => hook_event(out),
        GuardCommand::HookRun { program, args } => hook_run(&program, &args, out),
        GuardCommand::HookStop { program, args } => hook_stop(&program, &args, out),
        GuardCommand::StopAudit { session } => stop_audit(session, out),
    }
}

fn hook_event<W: Write>(out: &mut W) -> Result<ExitCode> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| Error::IoFailure {
            path: PathBuf::from("(stdin)"),
            source: e,
        })?;
    let event = HookEvent::from_stdin_json(&buf)?;
    write_envelope_success(out, event)?;
    Ok(ExitCode::SUCCESS)
}

fn hook_run<W: Write>(program: &str, args: &[String], out: &mut W) -> Result<ExitCode> {
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let outcome = HookRunner::run(program, &arg_refs)?;
    let exit = match outcome {
        HookRunOutcome::Completed { exit_code } => {
            let code: u8 = if (0..=255).contains(&exit_code) {
                exit_code as u8
            } else {
                1
            };
            ExitCode::from(code)
        }
        HookRunOutcome::SkippedFailOpen => ExitCode::SUCCESS,
        HookRunOutcome::StopForcedSuccess { .. } => ExitCode::SUCCESS,
    };
    write_envelope_success(out, outcome)?;
    Ok(exit)
}

fn hook_stop<W: Write>(program: &str, args: &[String], out: &mut W) -> Result<ExitCode> {
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let outcome = HookRunner::run_stop(program, &arg_refs)?;
    // Stop hook contract: ALWAYS exit 0. The observed exit code lives in
    // the envelope payload for telemetry but never propagates to git.
    write_envelope_success(out, outcome)?;
    Ok(ExitCode::SUCCESS)
}

fn stop_audit<W: Write>(session: Option<String>, out: &mut W) -> Result<ExitCode> {
    let (config, config_path, working_dir) = load_config()?;
    let sa_cfg = config
        .guard
        .as_ref()
        .and_then(|g| g.stop_audit.as_ref())
        .ok_or_else(|| Error::ConfigInvalid {
            message: "no [guard.stop_audit] section in harness.toml".into(),
            location: None,
        })?;
    let root = config_dir(&config_path, &working_dir);

    let session_id = match session {
        Some(s) => s,
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| Error::IoFailure {
                    path: PathBuf::from("(stdin)"),
                    source: e,
                })?;
            HookEvent::from_stdin_json(&buf)?.session_id
        }
    };

    let auditor = StopAuditor::new(sa_cfg, &root, session_id);
    let decision = auditor.run()?;
    // Stop-hook contract: exit 2 prevents the stop and forces continuation;
    // exit 1 would be non-blocking (the Block would have no effect). The
    // bounded retry counter inside StopAuditor keeps this from looping.
    let exit = match decision {
        StopDecision::Allow => ExitCode::SUCCESS,
        StopDecision::Block { .. } => ExitCode::from(2),
    };
    write_envelope_success(out, decision)?;
    Ok(exit)
}
