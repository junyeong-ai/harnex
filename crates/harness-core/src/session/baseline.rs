//! # baseline — a window's rates, kept so a later window can be held against it
//!
//! The transcripts are the operator's whole history, so re-reading them next
//! month re-reads the same months. A window is frozen into a [`Baseline`] and
//! the next window is compared to that record rather than to a fresh pass over
//! everything.
//!
//! Every metric in [`SessionMetric`] is a rate whose denominator is its own
//! sample — per submission, per commit, per stop. That is what makes the
//! comparison readable in both directions: a share-of-characters figure is
//! unchanged when the operator writes twice as much and repeats twice as much,
//! and a ratio whose denominator is not a count of observations gives [`diff`]
//! nothing to withhold on.
//!
//! ## What this module refuses to do
//!
//! - Never store what was written. A baseline holds counts and a span; the
//!   text stays in the transcripts.
//! - Never compare overlapping windows. Measuring everything, changing
//!   something, and measuring everything again dilutes the delta by whatever
//!   history preceded the change — a near-zero that reads as "nothing
//!   improved" and means "the question was not asked".
//! - Never fill in a metric one side does not carry, and never call a delta an
//!   effect: the runtime versions of both windows ride along for that reason.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::path_guard;
use crate::session::SessionFacts;
use crate::session::record::Coverage;
use crate::session::repository::HarnessState;
use crate::wire_enum::wire_enum;

/// A rate, kept as the two counts it came from.
///
/// `denominator` is both the divisor and the sample size, which is what lets a
/// consumer decide whether the rate is worth reading without a second field to
/// keep in step with the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Measurement {
    pub numerator: u64,
    pub denominator: u64,
}

impl Measurement {
    fn new(numerator: usize, denominator: usize) -> Self {
        Self {
            numerator: numerator as u64,
            denominator: denominator as u64,
        }
    }

    /// `None` when nothing was observed. A rate over an empty population is
    /// not zero.
    pub fn rate(&self) -> Option<f64> {
        (self.denominator > 0).then(|| self.numerator as f64 / self.denominator as f64)
    }

    /// Whether a comparison can subtract this from another measurement.
    ///
    /// The floor is raised to one whatever the configuration says: an empty
    /// population has no rate to compare, so a floor of zero would admit a
    /// subtraction there is nothing to subtract.
    pub fn supports(&self, support_floor: u64) -> bool {
        self.denominator >= support_floor.max(1)
    }
}

wire_enum! {
    /// The rates a baseline carries.
    ///
    /// Closed, because a baseline written by one build is read by another: the
    /// wire names are the join between them, and an exhaustive match is what
    /// keeps a new variant from being measured on one side of a comparison
    /// only. A metric whose definition changes is renamed rather than
    /// redefined, so an older baseline lands in `metrics_unmatched` instead of
    /// being compared against a number that no longer means the same thing.
    ///
    /// Every rate is denominated in what causes it, so a window that did more
    /// work does not read as a window that went worse.
    pub enum SessionMetric {
        /// Characters written again in a session that did not yet hold them —
        /// text no harness was holding.
        CrossSessionCharsPerSubmission => "cross_session_chars_per_submission",
        /// Characters written again inside a session that already held them —
        /// text that was in context and did not survive it.
        WithinSessionCharsPerSubmission => "within_session_chars_per_submission",
        /// Characters of project memory the runtime loaded.
        RuleLoadCharsPerSubmission => "rule_load_chars_per_submission",
        /// Tool calls a permission rule or the operator stopped.
        DenialsPerSubmission => "denials_per_submission",
        /// Instructions the operator sent without waiting for the agent that
        /// was already answering the previous one.
        SteeringPerSubmission => "steering_per_submission",
        /// Files edited again after a commit and before the next one.
        /// Denominated in observed commits, which is a floor, so this reads
        /// high; compare it only against another window measured the same way.
        ReeditsPerCommit => "reedits_per_commit",
        /// Wall-clock the Stop hooks held.
        HookMillisecondsPerStop => "hook_milliseconds_per_stop",
        /// Tokens the agent generated. Read beside the window's model set: a
        /// mix that moved moves this for a reason that is not the operator.
        OutputTokensPerSubmission => "output_tokens_per_submission",
    }
}

impl SessionMetric {
    /// Read this metric off a window's facts.
    ///
    /// `None` where the window cannot answer this metric at all, which is not
    /// the same as answering zero: an unrecorded metric lands in
    /// [`BaselineDiff::metrics_unmatched`], where a recorded zero would be
    /// subtracted as though it had been measured.
    pub fn measure(self, facts: &SessionFacts) -> Option<Measurement> {
        let submissions = facts.prompts.submissions;
        Some(match self {
            Self::CrossSessionCharsPerSubmission => {
                Measurement::new(facts.prompts.across_sessions.as_ref()?.chars, submissions)
            }
            Self::WithinSessionCharsPerSubmission => {
                Measurement::new(facts.prompts.within_sessions.chars, submissions)
            }
            Self::RuleLoadCharsPerSubmission => Measurement::new(
                facts.harness.rule_loads.iter().map(|r| r.chars).sum(),
                submissions,
            ),
            Self::DenialsPerSubmission => Measurement::new(
                facts.harness.denials.iter().map(|d| d.denials).sum(),
                submissions,
            ),
            Self::SteeringPerSubmission => Measurement::new(
                facts
                    .interventions
                    .by_kind
                    .get(crate::session::InterventionKind::Steering.as_str())
                    .copied()
                    .unwrap_or(0),
                submissions,
            ),
            Self::ReeditsPerCommit => Measurement::new(
                facts
                    .rework
                    .post_commit_reedits
                    .iter()
                    .map(|r| r.reedits)
                    .sum(),
                facts.rework.commits,
            ),
            Self::HookMillisecondsPerStop => Measurement {
                numerator: facts.harness.hooks.iter().map(|h| h.total_ms).sum(),
                denominator: facts.harness.stops as u64,
            },
            Self::OutputTokensPerSubmission => Measurement {
                numerator: facts.tokens.output,
                denominator: submissions as u64,
            },
        })
    }
}

/// Whether the harness moved between the two windows a comparison holds
/// against each other.
///
/// A delta across an unchanged harness is a delta about something else — the
/// work, the model, the runtime, the operator. That is the question a
/// before-and-after is asked, and nothing else in the record answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessChange {
    /// Both windows name the same commit and neither had uncommitted changes.
    Unchanged,
    /// The windows name different commits.
    Changed,
    /// A window recorded no harness state, or had uncommitted changes, so what
    /// it ran under is not identified by a commit.
    Unknown,
}

impl HarnessChange {
    pub const ALL: &'static [Self] = &[Self::Unchanged, Self::Changed, Self::Unknown];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "unchanged" => Self::Unchanged,
            "changed" => Self::Changed,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Changed => "changed",
            Self::Unknown => "unknown",
        }
    }

    fn between(from: Option<&HarnessState>, to: Option<&HarnessState>) -> Self {
        match (from, to) {
            (Some(a), Some(b)) if !a.uncommitted && !b.uncommitted && a.head.is_some() => {
                match a.head == b.head {
                    true => Self::Unchanged,
                    false => Self::Changed,
                }
            }
            (Some(a), Some(b)) if a.head != b.head => Self::Changed,
            _ => Self::Unknown,
        }
    }
}

/// What a window was measured under, beside what it measured.
///
/// A baseline is read months after it was written, by a build that may compute
/// its metrics differently and against a harness that has moved since. These
/// are what let that reader say whether the two numbers are comparable.
pub struct Measured<'a> {
    pub label: &'a str,
    pub recorded_at: Timestamp,
    /// The project this window was scoped to, if it was scoped to one.
    pub project: Option<PathBuf>,
    /// `[session] min_block_chars` as this window was measured.
    pub min_block_chars: usize,
    /// The harness the project carried at this moment, where it could be asked.
    pub harness: Option<HarnessState>,
}

/// One window, as it was measured.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Baseline {
    /// Operator-chosen name, unique in the ledger. It is how a later
    /// comparison names this window, so the ledger refuses to reuse one.
    pub label: String,
    pub recorded_at: Timestamp,
    /// The project this window was measured under, if it was scoped to one.
    /// Two windows of different scope describe different populations, so
    /// [`diff`] refuses to subtract one from the other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<PathBuf>,
    /// What the reader saw and what it could not — including the observed
    /// span, which is what makes two baselines comparable or not.
    pub coverage: Coverage,
    /// The build that measured this window. A metric whose definition moved
    /// between two builds is a delta about the definition, for the same reason
    /// the runtime versions and the model set ride along.
    ///
    /// `None` on a baseline written before this was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle_version: Option<String>,
    /// `[session] min_block_chars` as this window was measured. The shortest
    /// paragraph that counts as one, so the repetition metrics mean something
    /// different either side of a change to it — the operator's half of what
    /// `oracle_version` says about the build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_block_chars: Option<usize>,
    /// The harness the window ran under. Absent where the window was not
    /// scoped to a project, or that project is not a git work tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<HarnessState>,
    /// Keyed by [`SessionMetric::as_str`]. A metric the window could not
    /// measure is absent rather than zero.
    pub measurements: BTreeMap<String, Measurement>,
}

impl Baseline {
    pub fn of(measured: Measured<'_>, facts: &SessionFacts) -> Self {
        Self {
            label: measured.label.to_string(),
            recorded_at: measured.recorded_at,
            project: measured.project,
            coverage: facts.coverage.clone(),
            oracle_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            min_block_chars: Some(measured.min_block_chars),
            harness: measured.harness,
            measurements: SessionMetric::ALL
                .iter()
                .filter_map(|m| Some((m.as_str().to_string(), m.measure(facts)?)))
                .collect(),
        }
    }

    /// The rates [`diff`] will withhold on either side of a comparison.
    ///
    /// A window can be too thin to say anything and still record cleanly, so
    /// this is what the operator is owed at the moment the baseline is written
    /// rather than at the moment a comparison against it comes back empty.
    pub fn unsupported(&self, support_floor: u64) -> Vec<&str> {
        self.measurements
            .iter()
            .filter(|(_, m)| !m.supports(support_floor))
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

/// Append-only ledger of baselines, in save order.
pub struct BaselineLedger {
    path: PathBuf,
}

impl BaselineLedger {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Record a window under a label no earlier record used.
    ///
    /// Uniqueness is enforced here rather than resolved at read time: a second
    /// `before` would silently change what an already-written comparison
    /// means, and the ledger is the only place that can see the collision.
    pub fn append(&self, baseline: &Baseline) -> Result<()> {
        if baseline.label.trim().is_empty() {
            return Err(Error::SessionBaselineLabelRejected {
                label: baseline.label.clone(),
                message: "a baseline is named so a later comparison can ask for it".into(),
            });
        }
        if let Some(prior) = self
            .load_all()?
            .into_iter()
            .find(|b| b.label == baseline.label)
        {
            return Err(Error::SessionBaselineLabelRejected {
                label: baseline.label.clone(),
                message: format!("already recorded at {}", prior.recorded_at),
            });
        }
        let line = serde_json::to_string(baseline).map_err(|e| self.corrupt(e.to_string()))?;
        path_guard::append_line(&self.path, line.as_bytes())
    }

    /// Every baseline, oldest first. A ledger that is not there is empty.
    pub fn load_all(&self) -> Result<Vec<Baseline>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&self.path).map_err(|e| Error::IoFailure {
            path: self.path.clone(),
            source: e,
        })?;
        let mut out = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            // A skipped line is a baseline that quietly stops existing, and the
            // label that named it starts resolving to a different window.
            let record = serde_json::from_str::<Baseline>(line)
                .map_err(|e| self.corrupt(format!("baseline line {}: {e}", idx + 1)))?;
            out.push(record);
        }
        Ok(out)
    }

    fn corrupt(&self, message: String) -> Error {
        Error::IoFailure {
            path: self.path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, message),
        }
    }
}

/// What a comparison must state about each side for its numbers to be read.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BaselineWindow {
    pub label: String,
    pub recorded_at: Timestamp,
    pub observed_from: Option<Timestamp>,
    pub observed_to: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<PathBuf>,
    /// The build that measured this window, for the reason
    /// [`Baseline::oracle_version`] gives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle_version: Option<String>,
    /// The paragraph floor this window was measured under, for the reason
    /// [`Baseline::min_block_chars`] gives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_block_chars: Option<usize>,
    /// The harness this window ran under, for the reason
    /// [`Baseline::harness`] gives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<HarnessState>,
    /// Runtime versions the window spans. A delta across a version change is
    /// an observation about two different runtimes as much as about the
    /// operator.
    pub runtime_versions: BTreeSet<String>,
    /// Models the window spans, for the same reason the versions are here.
    pub models: BTreeSet<String>,
    pub authorship_ratio: Option<f64>,
}

impl BaselineWindow {
    fn of(baseline: &Baseline) -> Self {
        Self {
            label: baseline.label.clone(),
            recorded_at: baseline.recorded_at,
            observed_from: baseline.coverage.observed_from,
            observed_to: baseline.coverage.observed_to,
            project: baseline.project.clone(),
            oracle_version: baseline.oracle_version.clone(),
            min_block_chars: baseline.min_block_chars,
            harness: baseline.harness.clone(),
            runtime_versions: baseline.coverage.runtime_versions.clone(),
            models: baseline.coverage.models.clone(),
            authorship_ratio: baseline.coverage.authorship_ratio(),
        }
    }
}

/// One metric across two windows.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MetricDelta {
    pub metric: String,
    pub from: Measurement,
    pub to: Measurement,
    /// `to` rate minus `from` rate. Absent when either side rests on fewer
    /// observations than the configured floor — the two rates are still
    /// reported, and only the subtraction is withheld.
    pub change: Option<f64>,
}

/// Two windows, held against each other.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BaselineDiff {
    pub from: BaselineWindow,
    pub to: BaselineWindow,
    pub support_floor: u64,
    /// Whether the harness moved between the two windows, by
    /// [`HarnessChange::as_str`]. A delta measured across `unchanged` is not
    /// an effect of a harness change, and one across `unknown` cannot be said
    /// either way.
    pub harness_change: String,
    pub metrics: Vec<MetricDelta>,
    /// Metrics one side carries and the other does not, because the builds
    /// that wrote them measured different things. Named rather than filled in.
    pub metrics_unmatched: Vec<String>,
}

/// Compare a later window to an earlier one.
///
/// Refuses unless the later window starts at or after the earlier one ends.
/// The two must be different stretches of time for the difference between them
/// to be about anything.
pub fn diff(from: &Baseline, to: &Baseline, support_floor: u64) -> Result<BaselineDiff> {
    if from.project != to.project {
        return Err(Error::SessionBaselineNotComparable {
            message: format!(
                "'{}' and '{}' were measured over different scopes, so the difference between them is the scope",
                from.label, to.label
            ),
        });
    }
    let (Some(earlier_end), Some(later_start)) =
        (from.coverage.observed_to, to.coverage.observed_from)
    else {
        return Err(Error::SessionBaselineNotComparable {
            message: format!(
                "'{}' or '{}' observed no timestamped record, so neither window has a span",
                from.label, to.label
            ),
        });
    };
    if earlier_end > later_start {
        return Err(Error::SessionBaselineNotComparable {
            message: format!(
                "'{}' runs to {earlier_end} and '{}' starts at {later_start}, so the second window contains part of the first",
                from.label, to.label
            ),
        });
    }

    let mut metrics = Vec::new();
    let mut metrics_unmatched = Vec::new();
    let keys: BTreeSet<&String> = from
        .measurements
        .keys()
        .chain(to.measurements.keys())
        .collect();
    for key in keys {
        match (from.measurements.get(key), to.measurements.get(key)) {
            (Some(a), Some(b)) => {
                let supported = a.supports(support_floor) && b.supports(support_floor);
                let change = match (supported, a.rate(), b.rate()) {
                    (true, Some(a), Some(b)) => Some(b - a),
                    _ => None,
                };
                metrics.push(MetricDelta {
                    metric: key.clone(),
                    from: *a,
                    to: *b,
                    change,
                });
            }
            _ => metrics_unmatched.push(key.clone()),
        }
    }

    Ok(BaselineDiff {
        harness_change: HarnessChange::between(from.harness.as_ref(), to.harness.as_ref())
            .as_str()
            .to_string(),
        from: BaselineWindow::of(from),
        to: BaselineWindow::of(to),
        support_floor,
        metrics,
        metrics_unmatched,
    })
}

/// Resolve the pair a comparison runs over: the later window, and whatever
/// the ledger recorded immediately before it unless another label is named.
pub fn select<'a>(
    ledger: &'a [Baseline],
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(&'a Baseline, &'a Baseline)> {
    let index_of = |label: &str| -> Result<usize> {
        ledger.iter().position(|b| b.label == label).ok_or_else(|| {
            Error::SessionBaselineNotComparable {
                message: format!("no baseline is labelled '{label}'"),
            }
        })
    };
    let short = || Error::SessionBaselineNotComparable {
        message: format!(
            "a comparison needs two baselines and the ledger holds {}",
            ledger.len()
        ),
    };

    let to = match to {
        Some(label) => index_of(label)?,
        None => ledger.len().checked_sub(1).ok_or_else(short)?,
    };
    let from = match from {
        Some(label) => index_of(label)?,
        None => to.checked_sub(1).ok_or_else(short)?,
    };
    Ok((&ledger[from], &ledger[to]))
}

/// Where the next window of this scope starts if it is to continue where the
/// ledger stopped. A window scoped to one project resumes from that project's
/// last measurement, never from an unrelated one.
///
/// The instant after the last record rather than its instant: `--since` keeps
/// a record at the boundary, so resuming from the boundary itself would put it
/// in both windows, and [`diff`] would call two windows sharing a record
/// consecutive.
pub fn latest_observed_to(ledger: &[Baseline], project: Option<&Path>) -> Option<Timestamp> {
    ledger
        .iter()
        .filter(|b| b.project.as_deref() == project)
        .filter_map(|b| b.coverage.observed_to)
        .max()
        .and_then(|t| t.checked_add(jiff::SignedDuration::from_nanos(1)).ok())
}

#[cfg(test)]
mod metric_tests {
    use super::SessionMetric;

    #[test]
    fn from_str_round_trips_every_variant() {
        for m in SessionMetric::ALL {
            assert_eq!(SessionMetric::from_str(m.as_str()), Some(*m));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert_eq!(SessionMetric::from_str("tokens_per_dollar"), None);
    }
}
