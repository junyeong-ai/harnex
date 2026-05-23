use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::envelope::ListResponse;
use harness_core::error::{Error, Result};
use harness_core::lifecycle::{
    DecisionLedger, LifecycleDecisionRecorder, ObservationLedger, PromotionCandidateFinder,
    PromotionDecision, RetirementClassifier, RetirementSweeper, consumer_detector_for,
};
use harness_core::telemetry::{JsonlStorage, TelemetryQuery};

use super::{config_dir, load_config, write_envelope_success};

#[derive(Subcommand)]
pub enum LifecycleCommand {
    /// Append an observation to the ledger
    Observe {
        #[arg(long)]
        tag: String,
        #[arg(long)]
        text: String,
        #[arg(long)]
        source: String,
    },
    /// List promotion candidates that crossed configured thresholds
    Candidates,
    /// Record an Approved decision — pattern promoted to a rule
    Promote(DecisionArgs),
    /// Record a Rejected decision — pattern declined
    Reject(DecisionArgs),
    /// Record a Deferred decision — pattern suspended pending more evidence
    Defer(DecisionArgs),
    /// Record a Demoted decision — previously approved pattern retracted
    /// (requires prior Approved decision for the same pattern)
    Demote(DecisionArgs),
    /// Classify a path under retirement signals (stale/no-consumers/silent)
    Classify {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        path: PathBuf,
        /// Whether telemetry reports zero events for this slug in the silence window
        #[arg(long)]
        silent: bool,
    },
    /// Sweep every kind × consumer detector, deriving Silent automatically
    /// from the telemetry ledger. Returns aggregate retirement verdicts.
    Retire {
        /// Override `[lifecycle].silence_window_days` for this run.
        #[arg(long)]
        window: Option<u32>,
    },
    /// List every decision recorded in the ledger (promote/reject/defer/demote),
    /// optionally filtered by tag and/or decision kind.
    Decisions {
        #[arg(long)]
        tag: Option<String>,
        /// Filter by decision kind (approved | rejected | deferred | demoted)
        #[arg(long, value_parser = decision_kind_values())]
        decision: Option<String>,
    },
}

#[derive(clap::Args)]
pub struct DecisionArgs {
    #[arg(long)]
    pub tag: String,
    #[arg(long)]
    pub text: String,
    /// Mandatory: human-authored rationale. Empty values are rejected.
    #[arg(long)]
    pub decision_text: String,
}

/// Source of truth for `--decision` clap value_parser — derives directly
/// from [`PromotionDecision::ALL`] so adding a variant auto-updates the CLI.
fn decision_kind_values() -> Vec<&'static str> {
    PromotionDecision::ALL.iter().map(|d| d.as_str()).collect()
}

pub fn run<W: Write>(cmd: LifecycleCommand, out: &mut W) -> Result<ExitCode> {
    let (config, config_path, working_dir) = load_config()?;
    let lc = config
        .lifecycle
        .as_ref()
        .ok_or_else(|| Error::ConfigInvalid {
            message: "no [lifecycle] section in harness.toml".into(),
            location: None,
        })?;
    let root = config_dir(&config_path, &working_dir);
    let resolve = |p: &PathBuf| -> PathBuf {
        if p.is_absolute() {
            p.clone()
        } else {
            root.join(p)
        }
    };
    let ledger = ObservationLedger::new(resolve(&lc.observation_dir));
    let decisions = DecisionLedger::new(resolve(&lc.decision_dir));
    let finder = PromotionCandidateFinder::new(lc, &ledger, &decisions);
    let recorder = LifecycleDecisionRecorder::new(&decisions);

    match cmd {
        LifecycleCommand::Observe { tag, text, source } => {
            let obs = ledger.append(&tag, &text, &source)?;
            write_envelope_success(out, obs)?;
            Ok(ExitCode::SUCCESS)
        }
        LifecycleCommand::Candidates => {
            let candidates = finder.list_candidates()?;
            write_envelope_success(out, ListResponse::new(candidates))?;
            Ok(ExitCode::SUCCESS)
        }
        LifecycleCommand::Promote(args) => emit_record(
            out,
            recorder.promote(&args.tag, &args.text, &args.decision_text)?,
        ),
        LifecycleCommand::Reject(args) => emit_record(
            out,
            recorder.reject(&args.tag, &args.text, &args.decision_text)?,
        ),
        LifecycleCommand::Defer(args) => emit_record(
            out,
            recorder.defer(&args.tag, &args.text, &args.decision_text)?,
        ),
        LifecycleCommand::Demote(args) => emit_record(
            out,
            recorder.demote(&args.tag, &args.text, &args.decision_text)?,
        ),
        LifecycleCommand::Retire { window } => {
            let tcfg = config
                .telemetry
                .as_ref()
                .ok_or_else(|| Error::ConfigInvalid {
                    message: "no [telemetry] section in harness.toml — retirement Silent signal requires the ledger".into(),
                    location: None,
                })?;
            let storage_dir = if tcfg.storage_dir.is_absolute() {
                tcfg.storage_dir.clone()
            } else {
                root.join(&tcfg.storage_dir)
            };
            let storage = JsonlStorage::new(storage_dir, tcfg.rotate_at_mb);
            let query = TelemetryQuery::new(storage);
            let mut sweep = RetirementSweeper::new(&config, &root, &query)?;
            if let Some(w) = window {
                sweep = sweep.with_silence_window(w);
            }
            let outcome = sweep.run()?;
            write_envelope_success(out, outcome)?;
            Ok(ExitCode::SUCCESS)
        }
        LifecycleCommand::Decisions { tag, decision } => {
            let decision_filter = decision
                .as_deref()
                .map(|s| {
                    PromotionDecision::from_str(s).ok_or_else(|| Error::ConfigInvalid {
                        message: format!("unknown decision '{s}'"),
                        location: None,
                    })
                })
                .transpose()?;
            let mut records = decisions.load_all()?;
            records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            let filtered: Vec<_> = records
                .into_iter()
                .filter(|r| tag.as_deref().map(|t| r.tag == t).unwrap_or(true))
                .filter(|r| decision_filter.map(|d| r.decision == d).unwrap_or(true))
                .collect();
            write_envelope_success(out, ListResponse::new(filtered))?;
            Ok(ExitCode::SUCCESS)
        }
        LifecycleCommand::Classify {
            kind,
            path,
            silent,
        } => {
            let detector_decl = lc
                .consumer_detectors
                .iter()
                .find(|d| d.kind == kind)
                .ok_or_else(|| Error::ConfigInvalid {
                    message: format!(
                        "no [[lifecycle.consumer_detectors]] for kind '{kind}' in harness.toml"
                    ),
                    location: None,
                })?
                .clone();
            let detector = consumer_detector_for(detector_decl, &root)?;
            let classifier = RetirementClassifier::new(lc, config.retirement.as_ref());
            let target_path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            let verdict =
                classifier.classify(&kind, &target_path, detector.as_ref(), silent)?;
            write_envelope_success(out, verdict)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn emit_record<W: Write>(
    out: &mut W,
    record: harness_core::lifecycle::DecisionRecord,
) -> Result<ExitCode> {
    write_envelope_success(out, record)?;
    Ok(ExitCode::SUCCESS)
}
