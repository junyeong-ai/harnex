use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use harness_core::error::{Error, Result};
use harness_core::guard::{
    FloorAuditor, FloorDecision, HookEvent, HookRunOutcome, HookRunner, StopAuditor, StopDecision,
};

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
    /// PreToolUse floor-integrity check (requires [guard.floor]). Wire
    /// directly for Bash and Edit|Write|MultiEdit. Blocks (exit 2, reason
    /// on stderr) only a detected violation — a hook-skipping git command,
    /// or a write to a protected path without the operator's standing
    /// override. Anything that prevents evaluation allows with a visible
    /// skip notice on the systemMessage channel.
    Floor,
}

pub fn run<W: Write>(cmd: GuardCommand, out: &mut W) -> Result<ExitCode> {
    match cmd {
        GuardCommand::HookEvent => hook_event(out),
        GuardCommand::HookRun { program, args } => hook_run(&program, &args, out),
        GuardCommand::HookStop { program, args } => hook_stop(&program, &args, out),
        GuardCommand::StopAudit { session } => stop_audit(session, out),
        GuardCommand::Floor => floor(out),
    }
}

/// The floor's hook contract, not the envelope: exit 2 is reserved for a
/// detected violation, whose reason feeds back on stderr; every other
/// outcome exits 0, with skip and grant surfaced on the `systemMessage`
/// channel. A config or stdin failure must never exit 2 here — a PreToolUse
/// non-zero exit blocks the agent's action, and inability to evaluate is
/// not a violation.
fn floor<W: Write>(out: &mut W) -> Result<ExitCode> {
    #[derive(serde::Deserialize)]
    struct FloorHookInput {
        #[serde(default)]
        tool_name: String,
        #[serde(default)]
        tool_input: serde_json::Value,
    }

    fn notice<W: Write>(out: &mut W, message: &str) -> Result<ExitCode> {
        let body = serde_json::json!({ "systemMessage": message, "suppressOutput": true });
        writeln!(out, "{body}").map_err(|e| Error::IoFailure {
            path: PathBuf::from("(stdout)"),
            source: e,
        })?;
        Ok(ExitCode::SUCCESS)
    }

    fn skip<W: Write>(out: &mut W, reason: &str) -> Result<ExitCode> {
        notice(out, &format!("[floor-check skipped: {reason}]"))
    }

    let mut buf = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
        return skip(out, &format!("stdin unreadable: {e}"));
    }
    let input: FloorHookInput = match serde_json::from_str(&buf) {
        Ok(parsed) => parsed,
        Err(e) => return skip(out, &format!("hook stdin json: {e}")),
    };
    let (config, config_path, working_dir) = match load_config() {
        Ok(loaded) => loaded,
        Err(e) => return skip(out, &e.to_string()),
    };
    let Some(floor_cfg) = config.guard.as_ref().and_then(|g| g.floor.as_ref()) else {
        return skip(
            out,
            "no [guard.floor] section in harness.toml — declare it or remove the PreToolUse wiring",
        );
    };
    let root = config_dir(&config_path, &working_dir);
    let auditor = FloorAuditor::new(floor_cfg);
    match auditor.evaluate(&root, &input.tool_name, &input.tool_input) {
        FloorDecision::Allow => Ok(ExitCode::SUCCESS),
        FloorDecision::Skip { reason } => skip(out, &reason),
        FloorDecision::Grant { path } => notice(
            out,
            &format!("[floor-edit allowed by the operator's standing override — {path}]"),
        ),
        FloorDecision::Block { reason } => {
            // The documented PreToolUse feedback channel: exit 2 blocks the
            // action and stderr reaches the agent.
            eprintln!("✗ floor-integrity: {reason}");
            Ok(ExitCode::from(2))
        }
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
