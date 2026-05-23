//! Two single-responsibility surfaces for the promotion lifecycle:
//!
//! - [`PromotionCandidateFinder`] — read-only. Aggregates observations
//!   into `(tag, normalized_text)` groups, surfaces those that crossed
//!   both `min_instances` and `min_days` thresholds AND have not been
//!   resolved with a suppressing decision (`Approved` / `Rejected` /
//!   `Demoted`). Operator-facing read of the candidate set.
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

/// Read-only listing of promotion candidates. Excludes any
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

    /// Surface every `(tag, normalized_text)` group that crossed BOTH
    /// thresholds AND has not been resolved with a suppressing decision.
    /// `Deferred` decisions are informational and do not suppress.
    pub fn list_candidates(&self) -> Result<Vec<PromotionCandidate>> {
        let observations = self.observations.load_all()?;
        let prior_decisions = self.decisions.load_all()?;
        let excluded: HashSet<(String, String)> = prior_decisions
            .into_iter()
            .filter(|d| d.decision.suppresses_resurfacing())
            .map(|d| (d.tag, d.normalized_text))
            .collect();

        let mut groups: HashMap<(String, String), Vec<Observation>> = HashMap::new();
        for o in observations {
            let key = (o.tag.clone(), normalize(&o.text));
            if excluded.contains(&key) {
                continue;
            }
            groups.entry(key).or_default().push(o);
        }

        let min_seconds = (self.config.promotion_min_days as i64) * 86400;
        let mut out = Vec::new();
        for ((tag, normalized_text), items) in groups {
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
            out.push(PromotionCandidate {
                tag,
                normalized_text,
                instance_count: count,
                span_days: elapsed / 86400,
                first_seen: first,
                last_seen: last,
                sources,
            });
        }
        out.sort_by_key(|c| std::cmp::Reverse(c.instance_count));
        Ok(out)
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
