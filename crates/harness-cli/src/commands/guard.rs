use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use harness_core::error::{Error, Result};
use harness_core::guard::{FloorAuditor, FloorDecision, OPERATOR_CHANNEL_KEY, SUPPRESS_OUTPUT_KEY};

use super::{config_dir, load_config};

#[derive(Subcommand)]
pub enum GuardCommand {
    /// PreToolUse floor-integrity check. Wire for Bash to refuse a
    /// hook-skipping git command, which needs no declaration, and for
    /// Edit|Write|MultiEdit to freeze what [guard.floor] declares. Blocks
    /// (exit 2, reason on stderr) only a detected violation. Anything that
    /// prevents evaluation allows with a visible skip notice on the
    /// systemMessage channel.
    Floor,
    /// PostToolUse / PostToolUseFailure telemetry emit — record one
    /// harness_invocation event (the invoked element's slug + the outcome).
    /// Wire to both events with matcher Skill|Task|Agent, best `async`. Always
    /// exits 0 and emits nothing on stdout; any reason it cannot record (no
    /// `[telemetry]`, the Kind undeclared, a write failure) is a silent no-op.
    TelemetryEmit,
}

pub fn run<W: Write>(cmd: GuardCommand, out: &mut W) -> Result<ExitCode> {
    match cmd {
        GuardCommand::Floor => floor(out),
        GuardCommand::TelemetryEmit => telemetry_emit(),
    }
}

/// Read the hook event from stdin and emit one harness_invocation event. This
/// speaks the install-to-enable, never-block contract: it writes no envelope,
/// and every reason it cannot record is a silent exit 0 — telemetry must never
/// interrupt the tool call that triggered it.
fn telemetry_emit() -> Result<ExitCode> {
    use harness_core::guard::telemetry;
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return Ok(ExitCode::SUCCESS);
    }
    let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let _ = telemetry::emit(&working_dir, &buf);
    Ok(ExitCode::SUCCESS)
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
    // Configuration is a precondition of the freeze alone. The tripwire reads
    // none, so a project whose `harness.toml` is gone keeps it — otherwise
    // removing that file, which no Edit can do but any Bash call can, would
    // take the tripwire with it.
    let auditor = match load_config() {
        Ok((config, config_path, working_dir)) => {
            match config.guard.as_ref().and_then(|g| g.floor.as_ref()) {
                Some(floor_cfg) => {
                    FloorAuditor::declared(config_dir(&config_path, &working_dir), floor_cfg)
                }
                None => FloorAuditor::undeclared(
                    "`harness.toml` declares no `[guard.floor]`, so no path is frozen — \
                     declare the section, or remove the Edit/Write PreToolUse wiring",
                ),
            }
        }
        Err(e) => FloorAuditor::undeclared(e.to_string()),
    };
    match auditor.evaluate(&input.tool_name, &input.tool_input) {
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

fn notice<W: Write>(out: &mut W, message: &str) -> Result<ExitCode> {
    let body = serde_json::json!({
        OPERATOR_CHANNEL_KEY: message,
        SUPPRESS_OUTPUT_KEY: true,
    });
    writeln!(out, "{body}").map_err(|e| Error::IoFailure {
        path: PathBuf::from("(stdout)"),
        source: e,
    })?;
    Ok(ExitCode::SUCCESS)
}
