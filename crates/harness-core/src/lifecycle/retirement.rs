//! Three-signal retirement classifier.
//!
//! - `Stale`: the file last changed more than `stale_days` ago, by the
//!   [`LastChange`] the caller derives from history
//! - `NoConsumers`: ConsumerDetector finds zero referencing files
//! - `Silent`: the telemetry ledger recorded a tracked invocation within
//!   `silence_window_days` and none names the slug. A ledger with no tracked
//!   invocation in the window yields [`SilenceState::Unmeasured`], not Silent
//!   — silence cannot be concluded from a ledger with nothing to be absent
//!   from, so it never counts as a signal. The caller derives the state; the
//!   classifier counts it.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::Serialize;

use crate::config::{LifecycleConfig, RetirementConfig};
use crate::envelope::Severity;
use crate::error::{Error, Result};
use crate::lifecycle::consumer::ConsumerDetector;
use crate::wire_enum::wire_enum;

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct RetirementOutcome {
    pub kind: String,
    pub slug: String,
    pub path: PathBuf,
    /// Days since the last commit that touched the file; 0 while the working
    /// tree holds a change no commit records.
    pub age_days: i64,
    pub consumer_count: usize,
    pub silence: SilenceState,
    pub signals: Vec<RetirementSignal>,
    pub severity: Severity,
    pub exempt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RetirementSignal {
    Stale,
    NoConsumers,
    Silent,
}

wire_enum! {
    /// The telemetry-derived silence verdict for one slug — the input the
    /// classifier turns into a `Silent` signal (or not).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
    #[serde(rename_all = "kebab-case")]
    pub enum SilenceState {
        /// Measured: the ledger recorded a tracked invocation in the window
        /// but none names this slug. The one state that fires `Silent`.
        Silent => "silent",
        /// Measured: an in-window event names this slug — the element is in use.
        Active => "active",
        /// The ledger recorded no tracked invocation in the window, so silence
        /// is unknown. Never a signal — an absent measurement is not a zero.
        Unmeasured => "unmeasured",
    }
}

/// When an artifact last changed, which `Stale` and the grace period read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastChange {
    /// The last commit that touched it; the working tree still matches it.
    Committed(Timestamp),
    /// A change no commit records yet — an edit in progress, or a file never
    /// committed — so the artifact is changing now.
    Uncommitted,
}

/// The [`LastChange`] of each of `paths`, in order, as the repository at
/// `working_dir` records it. One history walk answers them all.
pub fn last_changes(working_dir: &Path, paths: &[PathBuf]) -> Result<Vec<LastChange>> {
    let committed = crate::git::last_committed(working_dir, paths)
        .map_err(|crate::git::Failure(message)| Error::LifecycleGitFailure { message })?;
    Ok(committed
        .into_iter()
        .map(|at| at.map_or(LastChange::Uncommitted, LastChange::Committed))
        .collect())
}

pub struct RetirementClassifier<'a> {
    config: &'a LifecycleConfig,
    retirement: Option<&'a RetirementConfig>,
}

impl<'a> RetirementClassifier<'a> {
    pub fn new(config: &'a LifecycleConfig, retirement: Option<&'a RetirementConfig>) -> Self {
        Self { config, retirement }
    }

    pub fn classify(
        &self,
        kind: &str,
        path: &Path,
        consumer: &ConsumerDetector,
        silence: SilenceState,
        changed: LastChange,
    ) -> Result<RetirementOutcome> {
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        std::fs::metadata(path).map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        let age_days = match changed {
            LastChange::Committed(at) => {
                (Timestamp::now().duration_since(at).as_secs() / 86_400).max(0)
            }
            LastChange::Uncommitted => 0,
        };
        let in_grace = age_days < (self.config.grace_period_days as i64);

        let consumers = consumer.find_consumers(&slug)?;
        let consumer_count = consumers.len();

        let mut signals = Vec::new();
        if age_days > (self.config.stale_days as i64) {
            signals.push(RetirementSignal::Stale);
        }
        if consumer_count == 0 {
            signals.push(RetirementSignal::NoConsumers);
        }
        match silence {
            SilenceState::Silent => signals.push(RetirementSignal::Silent),
            // Active names the slug; Unmeasured recorded nothing to be absent
            // from. Neither concludes silence, so neither is a signal.
            SilenceState::Active | SilenceState::Unmeasured => {}
        }

        let severity = match signals.len() {
            3 => Severity::Major,
            2 => Severity::Minor,
            _ => Severity::Info,
        };

        let exempt = in_grace
            || self
                .retirement
                .map(|r| {
                    r.exempt.kinds.iter().any(|k| k == kind)
                        || r.exempt.slugs.iter().any(|s| s == &slug)
                })
                .unwrap_or(false);

        Ok(RetirementOutcome {
            kind: kind.to_string(),
            slug,
            path: path.to_path_buf(),
            age_days,
            consumer_count,
            silence,
            signals,
            severity,
            exempt,
        })
    }
}

#[cfg(test)]
mod strategy_tests {
    use super::SilenceState;

    #[test]
    fn from_str_round_trips_every_variant() {
        for s in SilenceState::ALL {
            assert_eq!(SilenceState::from_str(s.as_str()), Some(*s));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert_eq!(SilenceState::from_str("quiet"), None);
    }

    #[test]
    fn as_str_matches_serde_kebab_case() {
        // The CLI `--silence` value_parser reads `as_str`; the envelope
        // serializes through serde. The two must spell each state alike.
        for s in SilenceState::ALL {
            assert_eq!(
                serde_json::to_string(s).unwrap(),
                format!("{:?}", s.as_str())
            );
        }
    }
}
