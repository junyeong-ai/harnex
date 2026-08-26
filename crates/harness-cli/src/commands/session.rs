use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use jiff::Timestamp;

use harness_core::config::SessionConfig;
use harness_core::error::{Error, Result};
use harness_core::session::{self, Baseline, BaselineLedger, CollectOptions};

use super::{config_dir, load_config, write_envelope_success};

/// Which records a command reads. Every session command takes the same two,
/// so they are declared once and flattened rather than repeated per verb.
#[derive(Args, Clone)]
pub struct WindowArgs {
    /// Ignore records older than this RFC 3339 timestamp
    #[arg(long)]
    since: Option<String>,
    /// Read only sessions run in this directory or below it
    #[arg(long)]
    project: Option<PathBuf>,
}

impl WindowArgs {
    fn resolve(self) -> Result<(Option<Timestamp>, Option<PathBuf>)> {
        let since = self
            .since
            .map(|raw| {
                raw.parse().map_err(|e| Error::ConfigInvalid {
                    message: format!("--since '{raw}' is not an RFC 3339 timestamp: {e}"),
                    location: None,
                })
            })
            .transpose()?;
        // Resolved rather than taken as given: a project that is not there
        // filters every record away, and a run that read nothing must not
        // report it as a project with nothing in it.
        let project = self
            .project
            .map(|path| {
                path.canonicalize().map_err(|e| Error::ConfigInvalid {
                    message: format!("--project {}: {e}", path.display()),
                    location: None,
                })
            })
            .transpose()?;
        Ok((since, project))
    }

    fn options(self, with_text: bool, with_submissions: bool) -> Result<CollectOptions> {
        let (since, project) = self.resolve()?;
        Ok(CollectOptions {
            with_text,
            with_submissions,
            since,
            project,
        })
    }
}

#[derive(Subcommand)]
pub enum SessionCommand {
    /// Transcripts discovered under the configured roots, and what of them was read
    Index {
        #[command(flatten)]
        window: WindowArgs,
    },
    /// The fact ledger for one window: counts and citations, no judgement
    Facts {
        #[command(flatten)]
        window: WindowArgs,
        /// Include prompt text alongside its citations
        #[arg(long)]
        with_text: bool,
    },
    /// Instructions one at a time, with what followed each
    Submissions {
        #[command(flatten)]
        window: WindowArgs,
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
        /// Window start. Defaults to where the most recent baseline of the
        /// same scope stopped, so consecutive baselines never overlap.
        #[arg(long)]
        since: Option<String>,
        /// Measure only sessions run in this directory or below it
        #[arg(long)]
        project: Option<PathBuf>,
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
        SessionCommand::Index { window } => {
            let facts = session::collect(session_config, &window.options(false, false)?)?;
            write_envelope_success(out, facts.coverage)?;
        }
        SessionCommand::Facts { window, with_text } => {
            let facts = measure(session_config, &window.options(with_text, false)?)?;
            write_envelope_success(out, facts)?;
        }
        SessionCommand::Submissions {
            window,
            with_text,
            sample,
        } => {
            let facts = measure(session_config, &window.options(with_text, true)?)?;
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
                BaselineCommand::Save {
                    label,
                    since,
                    project,
                } => {
                    let (since, project) = WindowArgs { since, project }.resolve()?;
                    let recorded = ledger.load_all()?;
                    let since = since.or_else(|| {
                        session::baseline::latest_observed_to(&recorded, project.as_deref())
                    });
                    let facts = measure(
                        session_config,
                        &CollectOptions {
                            since,
                            project: project.clone(),
                            ..CollectOptions::default()
                        },
                    )?;
                    let baseline = Baseline::of(&label, Timestamp::now(), project, &facts);
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
