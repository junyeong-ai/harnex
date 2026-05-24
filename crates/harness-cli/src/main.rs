//! `harness` — JSON-first CLI wrapping `harness-core`.
//!
//! Every command emits exactly one envelope on stdout. Exit codes:
//! - 0 = success
//! - 1 = success with blocking findings / drift / mismatch
//! - 2 = runtime failure (unexpected error)

use std::io::{self, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use harness_core::envelope;

mod commands;

#[derive(Parser)]
#[command(
    name = "harness",
    version,
    about = "Harness engineering toolkit for Claude Code projects"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Verify provenance markers in markdown
    Evidence {
        #[command(subcommand)]
        cmd: commands::evidence::EvidenceCommand,
    },
    /// Append-only telemetry ledger
    Telemetry {
        #[command(subcommand)]
        cmd: commands::telemetry::TelemetryCommand,
    },
    /// Cross-file sentinel-block sync
    Codegen {
        #[command(subcommand)]
        cmd: commands::codegen::CodegenCommand,
    },
    /// Permission profiles + version pins
    Policy {
        #[command(subcommand)]
        cmd: commands::policy::PolicyCommand,
    },
    /// Frontmatter and structural checks for Claude Code surfaces
    Validate {
        #[command(subcommand)]
        cmd: commands::validate::ValidateCommand,
    },
    /// Promotion / retirement / observation ledger
    Lifecycle {
        #[command(subcommand)]
        cmd: commands::lifecycle::LifecycleCommand,
    },
    /// Claude Code runtime guards (hooks, Stop audit)
    Guard {
        #[command(subcommand)]
        cmd: commands::guard::GuardCommand,
    },
    /// Emit JSON Schema for the toolkit's user-facing types
    Export {
        #[command(subcommand)]
        cmd: commands::export::ExportCommand,
    },
    /// Read-only queries over a nodex document graph
    Graph {
        #[command(subcommand)]
        cmd: commands::graph::GraphCommand,
    },
    /// Emit shell completions (bash | zsh | fish | powershell | elvish)
    Completions(commands::completions::CompletionsArgs),
    /// Unified validation gate — runs every enabled validator
    Check(commands::check::CheckArgs),
    /// Harness-engineering compliance gate — spec drift + managed-region integrity
    Audit(commands::audit::AuditArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let result = match cli.command {
        Command::Evidence { cmd } => commands::evidence::run(cmd, &mut out),
        Command::Telemetry { cmd } => commands::telemetry::run(cmd, &mut out),
        Command::Codegen { cmd } => commands::codegen::run(cmd, &mut out),
        Command::Policy { cmd } => commands::policy::run(cmd, &mut out),
        Command::Validate { cmd } => commands::validate::run(cmd, &mut out),
        Command::Lifecycle { cmd } => commands::lifecycle::run(cmd, &mut out),
        Command::Guard { cmd } => commands::guard::run(cmd, &mut out),
        Command::Export { cmd } => commands::export::run(cmd, &mut out),
        Command::Graph { cmd } => commands::graph::run(cmd, &mut out),
        Command::Completions(args) => commands::completions::run(args, &mut out),
        Command::Check(args) => commands::check::run(args, &mut out),
        Command::Audit(args) => commands::audit::run(args, &mut out),
    };

    match result {
        Ok(exit) => exit,
        Err(e) => {
            let _ = envelope::write_error(&mut out, &e);
            let _ = out.flush();
            ExitCode::from(2)
        }
    }
}
