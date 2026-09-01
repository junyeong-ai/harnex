//! Two single-responsibility surfaces for the promotion lifecycle:
//!
//! - [`PromotionCandidateFinder`] — read-only. Aggregates observations
//!   into `(tag, normalized_text)` groups, surfaces those that crossed
//!   both `min_instances` and `min_days` thresholds AND have not been
//!   resolved with a suppressing decision (`Approved` / `Rejected` /
//!   `Demoted`), reported with the corpus they were drawn from
//!   ([`CandidateSurvey`]).
//!
//! - [`LifecycleDecisionRecorder`] — write-only (besides the demote
//!   prerequisite read). The four verbs `promote` / `reject` / `defer` /
//!   `demote` append to the decision ledger. `demote` enforces a state
//!   machine guard: it refuses unless the LATEST decision for the same
//!   `(tag, normalized_text)` is `Approved`.

use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use serde::Serialize;

use crate::config::LifecycleConfig;
use crate::error::{Error, Result};
use crate::lifecycle::decision::{DecisionLedger, DecisionRecord, PromotionDecision};
use crate::lifecycle::observation::{Observation, ObservationLedger};

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct PromotionCandidate {
    pub tag: String,
    pub normalized_text: String,
    pub instance_count: u32,
    pub span_days: i64,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    pub sources: Vec<String>,
}

/// The candidates, and the corpus they were drawn from.
///
/// A bare list cannot say which of two opposite things an empty result is: a
/// ledger nothing has been written to, or a corpus whose observations have not
/// recurred. The curate pass reads the first as a loop whose emit half never
/// fired and the second as a finished pass, so the counts are what makes the
/// zero readable at all.
///
/// Every observation falls in exactly one group, and every group is either
/// already resolved or was measured against the thresholds — so
/// `groups_considered + groups_resolved` is the whole ledger's distinct
/// groups, and `candidates` is the part of `groups_considered` that crossed.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct CandidateSurvey {
    /// Groups past both thresholds, most instances first.
    pub candidates: Vec<PromotionCandidate>,
    /// Observations the ledger held.
    pub observations_read: usize,
    /// Distinct `(tag, normalized_text)` groups the thresholds ran against.
    pub groups_considered: usize,
    /// Groups a suppressing decision had already closed, excluded before the
    /// thresholds ran.
    pub groups_resolved: usize,
}

/// Read-only survey of promotion candidates. Excludes any
/// `(tag, normalized_text)` pair already resolved with a suppressing
/// decision (`Approved` / `Rejected` / `Demoted`).
pub struct PromotionCandidateFinder<'a> {
    config: &'a LifecycleConfig,
    observations: &'a ObservationLedger,
    decisions: &'a DecisionLedger,
}

impl<'a> PromotionCandidateFinder<'a> {
    pub fn new(
        config: &'a LifecycleConfig,
        observations: &'a ObservationLedger,
        decisions: &'a DecisionLedger,
    ) -> Self {
        Self {
            config,
            observations,
            decisions,
        }
    }

    /// Survey the ledger: every `(tag, normalized_text)` group that crossed
    /// BOTH thresholds AND has not been resolved with a suppressing decision,
    /// beside the corpus the answer was computed over. `Deferred` decisions
    /// are informational and do not suppress.
    pub fn survey(&self) -> Result<CandidateSurvey> {
        let observations = self.observations.load_all()?;
        let prior_decisions = self.decisions.load_all()?;
        let resolved: HashSet<(String, String)> = prior_decisions
            .into_iter()
            .filter(|d| d.decision.suppresses_resurfacing())
            .map(|d| (d.tag, d.normalized_text))
            .collect();

        let observations_read = observations.len();
        let mut groups: HashMap<(String, String), Vec<Observation>> = HashMap::new();
        for o in observations {
            groups
                .entry((o.tag.clone(), normalize(&o.text)))
                .or_default()
                .push(o);
        }

        let min_seconds = (self.config.promotion_min_days as i64) * 86400;
        let mut groups_considered = 0;
        let mut groups_resolved = 0;
        let mut candidates = Vec::new();
        for (key, items) in groups {
            if resolved.contains(&key) {
                groups_resolved += 1;
                continue;
            }
            groups_considered += 1;
            let count = items.len() as u32;
            if count < self.config.promotion_min_instances {
                continue;
            }
            let mut first = items[0].timestamp;
            let mut last = items[0].timestamp;
            for item in items.iter().skip(1) {
                if item.timestamp < first {
                    first = item.timestamp;
                }
                if item.timestamp > last {
                    last = item.timestamp;
                }
            }
            let elapsed = last.duration_since(first).as_secs();
            if elapsed < min_seconds {
                continue;
            }
            let mut sources: Vec<String> = items.into_iter().map(|o| o.source).collect();
            sources.sort();
            sources.dedup();
            let (tag, normalized_text) = key;
            candidates.push(PromotionCandidate {
                tag,
                normalized_text,
                instance_count: count,
                span_days: elapsed / 86400,
                first_seen: first,
                last_seen: last,
                sources,
            });
        }
        // Groups arrive in hash order, so the tiebreak is what makes one
        // ledger answer one way: an envelope that reorders between two runs
        // over unchanged input is not a deterministic output (Article I).
        candidates.sort_by(|a, b| {
            b.instance_count
                .cmp(&a.instance_count)
                .then_with(|| a.tag.cmp(&b.tag))
                .then_with(|| a.normalized_text.cmp(&b.normalized_text))
        });
        Ok(CandidateSurvey {
            candidates,
            observations_read,
            groups_considered,
            groups_resolved,
        })
    }
}

/// Append human-authored decisions to the ledger. Each verb refuses
/// empty `decision_text` (AI never invents promotion text). `demote`
/// additionally enforces a state-machine guard.
pub struct LifecycleDecisionRecorder<'a> {
    decisions: &'a DecisionLedger,
}

impl<'a> LifecycleDecisionRecorder<'a> {
    pub fn new(decisions: &'a DecisionLedger) -> Self {
        Self { decisions }
    }

    /// Record an `Approved` decision — pattern promoted to a rule.
    pub fn promote(&self, tag: &str, text: &str, decision_text: &str) -> Result<DecisionRecord> {
        self.record(PromotionDecision::Approved, tag, text, decision_text)
    }

    /// Record a `Rejected` decision — pattern declined from rule status.
    pub fn reject(&self, tag: &str, text: &str, decision_text: &str) -> Result<DecisionRecord> {
        self.record(PromotionDecision::Rejected, tag, text, decision_text)
    }

    /// Record a `Deferred` decision — pattern suspended pending more evidence.
    /// Does not suppress future candidate surfacing.
    pub fn defer(&self, tag: &str, text: &str, decision_text: &str) -> Result<DecisionRecord> {
        self.record(PromotionDecision::Deferred, tag, text, decision_text)
    }

    /// Record a `Demoted` decision — previously approved pattern retracted.
    /// Refuses unless the LATEST decision for `(tag, normalized_text)` is
    /// `Approved`. A pattern already Demoted / Rejected / never Approved
    /// cannot be demoted — the operator must re-Approve (rehabilitation)
    /// first before another Demoted is accepted.
    pub fn demote(&self, tag: &str, text: &str, decision_text: &str) -> Result<DecisionRecord> {
        let normalized_text = normalize(text);
        let prior = self.decisions.load_all()?;
        let latest = prior
            .iter()
            .filter(|d| d.tag == tag && d.normalized_text == normalized_text)
            .max_by_key(|d| d.timestamp);
        match latest {
            Some(d) if d.decision == PromotionDecision::Approved => {}
            _ => {
                return Err(Error::LifecycleDemoteWithoutApproval {
                    tag: tag.to_string(),
                    normalized_text,
                });
            }
        }
        self.record(PromotionDecision::Demoted, tag, text, decision_text)
    }

    fn record(
        &self,
        decision: PromotionDecision,
        tag: &str,
        text: &str,
        decision_text: &str,
    ) -> Result<DecisionRecord> {
        if decision_text.trim().is_empty() {
            return Err(Error::LifecycleDecisionTextEmpty);
        }
        let record = DecisionRecord {
            tag: tag.to_string(),
            normalized_text: normalize(text),
            decision,
            decision_text: decision_text.to_string(),
            timestamp: Timestamp::now(),
        };
        self.decisions.append(&record)?;
        Ok(record)
    }
}

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
