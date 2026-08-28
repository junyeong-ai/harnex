use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use jiff::Timestamp;

use harness_core::config::SessionConfig;
use harness_core::envelope::Warning;
use harness_core::error::{Error, Result};
use harness_core::session::{self, Baseline, BaselineLedger, CollectOptions};

use super::{config_dir, load_config, write_envelope_success, write_envelope_success_warned};

/// Which records a command reads. Every session command takes the same three,
/// so they are declared once and flattened rather than repeated per verb.
#[derive(Args, Clone)]
pub struct WindowArgs {
    /// Ignore records older than this RFC 3339 timestamp
    #[arg(long)]
    since: Option<String>,
    /// Read only sessions run in this directory or below it
    #[arg(long)]
    project: Option<PathBuf>,
    /// Read only this session, subagents it dispatched included
    #[arg(long)]
    session: Option<String>,
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
        let session = self.session.clone();
        let (since, project) = self.resolve()?;
        Ok(CollectOptions {
            with_text,
            with_submissions,
            since,
            project,
            session,
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
        /// Include what was written — prompt text, and the input of a refused
        /// tool call. Both carry whatever the operator typed, so neither is in
        /// the default result.
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

/// The code a consumer matches on when a baseline records cleanly and cannot
/// anchor a comparison.
const THIN_WINDOW_CODE: &str = "SESSION_BASELINE_BELOW_SUPPORT";

/// The code a consumer matches on when part of the operator's own text was
/// written under a prompt source this binary does not recognise.
const UNREAD_TEXT_CODE: &str = "SESSION_COVERAGE_BELOW_FLOOR";

/// What the operator is owed when the window read only some of their writing.
///
/// The rates that read it are withheld and the rest answer, so the envelope
/// looks like an ordinary one — this is what says it is not. Naming the sources
/// matters more than the ratio: they are the vocabulary this binary has not
/// caught up with, and an operator who recognises one can say so upstream.
fn unread_text(coverage: &session::Coverage, floor: f64) -> Vec<Warning> {
    let Some(ratio) = coverage.authorship_ratio().filter(|r| *r < floor) else {
        return Vec::new();
    };
    vec![Warning {
        code: UNREAD_TEXT_CODE.to_string(),
        message: format!(
            "{ratio:.3} of the turns the runtime attributed to a person carried a prompt source this binary recognises, below session.coverage_floor {floor}; the rates taken over the operator's own text are not recorded and the rest are"
        ),
    }]
}

/// What the operator is owed about a window too thin to compare against.
///
/// Saving is not the failure — the ledger holds it and the next window starts
/// where it ended. Silence is: a baseline whose every rate is withheld looks
/// from the envelope exactly like one that will answer.
fn thin_window(baseline: &Baseline, support_floor: u64) -> Vec<Warning> {
    let unsupported = baseline.unsupported(support_floor);
    if unsupported.is_empty() {
        return Vec::new();
    }
    vec![Warning {
        code: THIN_WINDOW_CODE.to_string(),
        message: format!(
            "{} of {} rates in '{}' are measured over fewer than {support_floor} observations \
             (session.min_support), and a diff against this baseline withholds them",
            unsupported.len(),
            baseline.measurements.len(),
            baseline.label,
        ),
    }]
}

/// Collect a window and refuse to report rates the window does not support.
fn measure(config: &SessionConfig, options: &CollectOptions) -> Result<session::SessionFacts> {
    let facts = session::collect(config, options)?;
    session::require_coverage(&facts.coverage)?;
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
            let warnings = unread_text(&facts.coverage, session_config.coverage_floor);
            write_envelope_success_warned(out, facts, warnings)?;
        }
        SessionCommand::Submissions {
            window,
            with_text,
            sample,
        } => {
            let facts = measure(session_config, &window.options(with_text, true)?)?;
            if sample == Some(0) {
                return Err(Error::ConfigInvalid {
                    message: "--sample 0 is not a cap; omit it to return the window whole".into(),
                    location: None,
                });
            }
            let cap = sample.or(session_config.submission_sample);
            let warnings = unread_text(&facts.coverage, session_config.coverage_floor);
            write_envelope_success_warned(
                out,
                session::SubmissionWindow {
                    submissions: match cap {
                        Some(max) => session::systematic_sample(&facts.submissions, max),
                        None => facts.submissions,
                    },
                    coverage: facts.coverage,
                },
                warnings,
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
                    let (since, project) = WindowArgs {
                        since,
                        project,
                        // A baseline anchors a trajectory, which is the
                        // operator's and not one session's.
                        session: None,
                    }
                    .resolve()?;
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
                    let baseline = Baseline::of(
                        session::Measured {
                            label: &label,
                            recorded_at: Timestamp::now(),
                            min_block_chars: session_config.min_block_chars,
                            coverage_floor: session_config.coverage_floor,
                            harness: match &project {
                                Some(dir) => session::repository::harness_state(
                                    dir,
                                    &session_config.harness_paths,
                                )?,
                                None => None,
                            },
                            project,
                        },
                        &facts,
                    );
                    ledger.append(&baseline)?;
                    let mut warnings = unread_text(&facts.coverage, session_config.coverage_floor);
                    warnings.extend(thin_window(&baseline, session_config.min_support));
                    write_envelope_success_warned(out, baseline, warnings)?;
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
