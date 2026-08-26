use std::io::Write;
use std::process::ExitCode;

use clap::Subcommand;
use jiff::Timestamp;

use harness_core::config::SessionConfig;
use harness_core::error::{Error, Result};
use harness_core::session::{self, Baseline, BaselineLedger, CollectOptions};

use super::{config_dir, load_config, write_envelope_success};

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
    /// Instructions one at a time, with what followed each
    Submissions {
        /// Ignore records older than this RFC 3339 timestamp
        #[arg(long)]
        since: Option<String>,
        /// Include the text of each instruction
        #[arg(long)]
        with_text: bool,
        /// Return at most this many, evenly spaced across the window.
        /// Overrides `[session] submission_sample`.
        #[arg(long)]
        sample: Option<usize>,
    },
    /// Measured windows, and the difference between two of them
    Baseline {
        #[command(subcommand)]
        cmd: BaselineCommand,
    },
}

#[derive(Subcommand)]
pub enum BaselineCommand {
    /// Measure a window and record it under a label no earlier baseline used
    Save {
        /// How a later comparison asks for this window
        #[arg(long)]
        label: String,
        /// Window start, as an RFC 3339 timestamp. Defaults to where the most
        /// recent baseline stopped, so consecutive baselines never overlap.
        #[arg(long)]
        since: Option<String>,
    },
    /// Compare two recorded windows
    Diff {
        /// Earlier window. Defaults to the baseline recorded before `--to`.
        #[arg(long)]
        from: Option<String>,
        /// Later window. Defaults to the most recent baseline.
        #[arg(long)]
        to: Option<String>,
    },
}

fn timestamp(raw: Option<String>) -> Result<Option<Timestamp>> {
    raw.map(|raw| {
        raw.parse().map_err(|e| Error::ConfigInvalid {
            message: format!("--since '{raw}' is not an RFC 3339 timestamp: {e}"),
            location: None,
        })
    })
    .transpose()
}

/// Collect a window and refuse to report rates the window does not support.
fn measure(config: &SessionConfig, options: &CollectOptions) -> Result<session::SessionFacts> {
    let facts = session::collect(config, options)?;
    session::require_coverage(&facts.coverage, config.coverage_floor)?;
    Ok(facts)
}

pub fn run<W: Write>(cmd: SessionCommand, out: &mut W) -> Result<ExitCode> {
    let (config, config_path, working_dir) = load_config()?;
    let session_config = config
        .session
        .as_ref()
        .ok_or_else(|| Error::ConfigInvalid {
            message: "[session] is not configured; declare roots to read transcripts".into(),
            location: None,
        })?;

    match cmd {
        SessionCommand::Index { since } => {
            let options = CollectOptions {
                since: timestamp(since)?,
                ..CollectOptions::default()
            };
            let facts = session::collect(session_config, &options)?;
            write_envelope_success(out, facts.coverage)?;
        }
        SessionCommand::Facts { since, with_text } => {
            let options = CollectOptions {
                with_text,
                since: timestamp(since)?,
                ..CollectOptions::default()
            };
            write_envelope_success(out, measure(session_config, &options)?)?;
        }
        SessionCommand::Submissions {
            since,
            with_text,
            sample,
        } => {
            let options = CollectOptions {
                with_text,
                since: timestamp(since)?,
                with_submissions: true,
            };
            let facts = measure(session_config, &options)?;
            let cap = sample.or(session_config.submission_sample);
            write_envelope_success(
                out,
                match cap {
                    Some(max) => session::systematic_sample(&facts.submissions, max),
                    None => facts.submissions,
                },
            )?;
        }
        SessionCommand::Baseline { cmd } => {
            let ledger = BaselineLedger::new(
                config_dir(&config_path, &working_dir).join(&session_config.baseline_path),
            );
            match cmd {
                BaselineCommand::Save { label, since } => {
                    let recorded = ledger.load_all()?;
                    let since = match timestamp(since)? {
                        Some(explicit) => Some(explicit),
                        None => session::baseline::latest_observed_to(&recorded),
                    };
                    let facts = measure(
                        session_config,
                        &CollectOptions {
                            since,
                            ..CollectOptions::default()
                        },
                    )?;
                    let baseline = Baseline::of(&label, Timestamp::now(), &facts);
                    ledger.append(&baseline)?;
                    write_envelope_success(out, baseline)?;
                }
                BaselineCommand::Diff { from, to } => {
                    let recorded = ledger.load_all()?;
                    let (from, to) =
                        session::baseline::select(&recorded, from.as_deref(), to.as_deref())?;
                    let diff = session::baseline::diff(from, to, session_config.min_support)?;
                    write_envelope_success(out, diff)?;
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
