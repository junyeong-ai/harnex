use std::io::Write;
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::error::{Error, Result};
use harness_core::session::{self, CollectOptions};

use super::{load_config, write_envelope_success};

#[derive(Subcommand)]
pub enum SessionCommand {
    /// Transcripts discovered under the configured roots, and what of them was read
    Index {
        /// Ignore records older than this RFC 3339 timestamp
        #[arg(long)]
        since: Option<String>,
    },
    /// The fact ledger for one window: counts and citations, no judgement
    Facts {
        /// Ignore records older than this RFC 3339 timestamp
        #[arg(long)]
        since: Option<String>,
        /// Include prompt text alongside its citations
        #[arg(long)]
        with_text: bool,
    },
}

fn options(since: Option<String>, with_text: bool) -> Result<CollectOptions> {
    let since = since
        .map(|raw| {
            raw.parse().map_err(|e| Error::ConfigInvalid {
                message: format!("--since '{raw}' is not an RFC 3339 timestamp: {e}"),
                location: None,
            })
        })
        .transpose()?;
    Ok(CollectOptions { with_text, since })
}

pub fn run<W: Write>(cmd: SessionCommand, out: &mut W) -> Result<ExitCode> {
    let (config, _config_path, _working_dir) = load_config()?;
    let session_config = config
        .session
        .as_ref()
        .ok_or_else(|| Error::ConfigInvalid {
            message: "[session] is not configured; declare roots to read transcripts".into(),
            location: None,
        })?;

    match cmd {
        SessionCommand::Index { since } => {
            let facts = session::collect(session_config, &options(since, false)?)?;
            write_envelope_success(out, facts.coverage)?;
        }
        SessionCommand::Facts { since, with_text } => {
            let facts = session::collect(session_config, &options(since, with_text)?)?;
            session::require_coverage(&facts.coverage, session_config.coverage_floor)?;
            write_envelope_success(out, facts)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
