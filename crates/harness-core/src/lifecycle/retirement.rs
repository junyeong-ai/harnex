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
    /// tree holds a change no commit records; null when no history here
    /// records the file at all.
    pub age_days: Option<i64>,
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

pub use crate::git::LastChange;

/// The [`LastChange`] of each of `paths`, in order, as the repository at
/// `working_dir` records it — the age `Stale` and the grace period read. One
/// history walk answers them all.
pub fn last_changes(working_dir: &Path, paths: &[PathBuf]) -> Result<Vec<LastChange>> {
    crate::git::last_changes(working_dir, paths)
        .map_err(|crate::git::Failure(message)| Error::LifecycleGitFailure { message })
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
        // What no history here records has no age, so it is neither stale
        // nor new: no measurement is read as a zero.
        let age_days = match changed {
            LastChange::Committed(at) => {
                Some((Timestamp::now().duration_since(at).as_secs() / 86_400).max(0))
            }
            LastChange::Uncommitted => Some(0),
            LastChange::Unrecorded => None,
        };
        let in_grace = age_days.is_some_and(|age| age < self.config.grace_period_days as i64);

        let consumers = consumer.find_consumers(&slug)?;
        let consumer_count = consumers.len();

        let mut signals = Vec::new();
        if age_days.is_some_and(|age| age > self.config.stale_days as i64) {
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
mod silence_state_tests {
    use super::SilenceState;

    #[test]
    fn silence_state_spells_itself_the_same_way_twice() {
        // The CLI `--silence` value_parser reads `as_str`; the envelope
        // serializes through serde.
        for variant in SilenceState::ALL {
            assert_eq!(
                serde_json::to_string(variant).unwrap(),
                format!("{:?}", variant.as_str()),
                "`rename_all` and the wire string are two spellings of one name"
            );
        }
    }
}
