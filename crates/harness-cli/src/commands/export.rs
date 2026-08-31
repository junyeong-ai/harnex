use std::io::Write;
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::error::{Error, Result};
use harness_core::export::{SchemaTarget, schema_for};

use super::write_envelope_success;

#[derive(Subcommand)]
pub enum ExportCommand {
    /// Emit a JSON Schema for the named target (or "all" for the full bundle)
    Schema {
        /// A schema target; "all" is the full bundle
        #[arg(value_parser = schema_target_values())]
        target: String,
        /// Emit the bare schema (pretty-printed) — no envelope wrapper.
        /// Use this when committing a schema file to disk for IDE
        /// autocomplete; the default envelope shape is for programmatic
        /// consumers. Mirrors `harnex completions --raw`.
        #[arg(long)]
        raw: bool,
    },
}

/// Source of truth for the `target` value_parser — derives from
/// [`SchemaTarget::ALL`] so adding a target auto-updates the CLI.
fn schema_target_values() -> Vec<&'static str> {
    SchemaTarget::ALL.iter().map(|t| t.as_str()).collect()
}

pub fn run<W: Write>(cmd: ExportCommand, out: &mut W) -> Result<ExitCode> {
    match cmd {
        ExportCommand::Schema { target, raw } => {
            let parsed = SchemaTarget::from_str(&target).ok_or_else(|| Error::ConfigInvalid {
                message: format!("unknown schema target '{target}'"),
                location: None,
            })?;
            let schema = schema_for(parsed);
            if raw {
                let pretty =
                    serde_json::to_string_pretty(&schema).map_err(|e| Error::IoFailure {
                        path: std::path::PathBuf::from("<stdout>"),
                        source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
                    })?;
                writeln!(out, "{pretty}").map_err(|e| Error::IoFailure {
                    path: std::path::PathBuf::from("<stdout>"),
                    source: e,
                })?;
            } else {
                write_envelope_success(out, schema)?;
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
