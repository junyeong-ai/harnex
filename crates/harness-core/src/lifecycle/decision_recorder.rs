//! Two single-responsibility surfaces for the promotion lifecycle:
//!
//! - [`LedgerReader`] — read-only. Reads both ledgers once into
//!   `(tag, normalized_text)` groups and answers two questions over that one
//!   reading. [`survey`](LedgerReader::survey) is the groups that crossed
//!   both `min_instances` and `min_days` and were not closed by a suppressing
//!   decision (`Approved` / `Rejected` / `Demoted`), with the corpus they were
//!   drawn from ([`CandidateSurvey`]). [`live`](LedgerReader::live) is every
//!   group by tag — open ones whole, closed ones by wording and decision — for
//!   the reader who judges what the thresholds cannot ([`LiveObservations`]).
//!
//! - [`LifecycleDecisionRecorder`] — write-only (besides the demote
//!   prerequisite read). The four verbs `promote` / `reject` / `defer` /
//!   `demote` append to the decision ledger. `demote` enforces a state
//!   machine guard: it refuses unless the LATEST decision for the same
//!   `(tag, normalized_text)` is `Approved`.

use std::collections::HashMap;

use jiff::Timestamp;
use serde::Serialize;

use crate::config::LifecycleConfig;
use crate::error::{Error, Result};
use crate::lifecycle::decision::{DecisionLedger, DecisionRecord, PromotionDecision};
use crate::lifecycle::observation::{Observation, ObservationLedger};

/// One standing wording under a tag: the observations that share a
/// `(tag, normalized_text)`, with how often, over what span, and from where
/// they were recorded.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ObservationGroup {
    pub tag: String,
    pub normalized_text: String,
    pub instance_count: u32,
    pub span_days: i64,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    /// Distinct sources, sorted.
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
///
/// The survey reads two ledgers and reports both, because `groups_resolved`
/// alone cannot say whether no pass has ever closed anything or the decision
/// ledger was not found: a relocated `decision_dir` silently resurfaces every
/// candidate the operator already rejected.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct CandidateSurvey {
    /// Groups past both thresholds, most instances first.
    pub candidates: Vec<ObservationGroup>,
    /// Observations the ledger held.
    pub observations_read: usize,
    /// Decisions the ledger held, of every kind.
    pub decisions_read: usize,
    /// Distinct `(tag, normalized_text)` groups the thresholds ran against.
    pub groups_considered: usize,
    /// Groups a suppressing decision had already closed, excluded before the
    /// thresholds ran.
    pub groups_resolved: usize,
}

/// Every wording the ledger holds, by tag: open ones whole, closed ones by
/// wording and decision.
///
/// The thresholds count recurrence per exact wording, so the same claim
/// recorded in two wordings is two groups under the bar — a recurrence the
/// survey cannot see, and what this layout exists to show. A tag's breadth is
/// what the promotion rubric counts, so tags come widest first, and the groups
/// under one are the wordings a new observation should reuse.
///
/// Read as the survey is: `observations_read` says whether the ledger was
/// written at all, `decisions_read` whether the ledger that closes groups was
/// found.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct LiveObservations {
    /// Every tag the ledger holds: by distinct sources over its live groups
    /// descending, then tag.
    pub tags: Vec<TagObservations>,
    /// Observations the ledger held.
    pub observations_read: usize,
    /// Decisions the ledger held, of every kind.
    pub decisions_read: usize,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TagObservations {
    pub tag: String,
    /// Distinct sources across the tag's open groups, sorted: the independent
    /// contexts the tag has been observed from.
    pub sources: Vec<String>,
    /// Open groups, most instances first.
    pub groups: Vec<ObservationGroup>,
    /// Wordings under this tag a suppressing decision closed, by wording. A
    /// sighting recorded under one joins its closed group and surfaces
    /// nowhere, which is what a settled decision means — so this is what a
    /// new observation is checked against before it is recorded.
    pub resolved: Vec<ResolvedGroup>,
}

/// A wording a suppressing decision closed, and the decision — the latest
/// suppressing one, where a wording has been closed more than once.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ResolvedGroup {
    pub normalized_text: String,
    pub decision: PromotionDecision,
}

/// One reading of both ledgers; the survey and the live layout are its two
/// projections, so their counts close against each other.
struct Reading {
    observations_read: usize,
    decisions_read: usize,
    /// Every group no suppressing decision has closed.
    live: Vec<ObservationGroup>,
    /// Every group one has, with its tag.
    resolved: Vec<(String, ResolvedGroup)>,
}

/// Read-only view of the two ledgers. A `(tag, normalized_text)` pair a
/// suppressing decision (`Approved` / `Rejected` / `Demoted`) has closed is
/// never a candidate and never open.
pub struct LedgerReader<'a> {
    config: &'a LifecycleConfig,
    observations: &'a ObservationLedger,
    decisions: &'a DecisionLedger,
}

impl<'a> LedgerReader<'a> {
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
        let reading = self.read()?;
        let groups_considered = reading.live.len();
        let min_days = i64::from(self.config.promotion_min_days);
        let mut candidates: Vec<ObservationGroup> = reading
            .live
            .into_iter()
            .filter(|group| {
                group.instance_count >= self.config.promotion_min_instances
                    && group.span_days >= min_days
            })
            .collect();
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
            observations_read: reading.observations_read,
            decisions_read: reading.decisions_read,
            groups_considered,
            groups_resolved: reading.resolved.len(),
        })
    }

    /// Every group the ledger holds, laid out by tag: open ones whole, closed
    /// ones by wording and decision. A tag with nothing open is still listed,
    /// because its closed wordings are what a new sighting under it is
    /// checked against.
    pub fn live(&self) -> Result<LiveObservations> {
        let reading = self.read()?;
        let mut by_tag: HashMap<String, TagObservations> = HashMap::new();
        for group in reading.live {
            let tag = tag_entry(&mut by_tag, &group.tag);
            tag.sources.extend(group.sources.iter().cloned());
            tag.groups.push(group);
        }
        for (tag, group) in reading.resolved {
            tag_entry(&mut by_tag, &tag).resolved.push(group);
        }
        // Tags and groups arrive in hash order; each sort below is a total
        // order, which is what makes one ledger answer one way (Article I).
        let mut tags: Vec<TagObservations> = by_tag
            .into_values()
            .map(|mut tag| {
                tag.sources.sort();
                tag.sources.dedup();
                tag.groups.sort_by(|a, b| {
                    b.instance_count
                        .cmp(&a.instance_count)
                        .then_with(|| a.normalized_text.cmp(&b.normalized_text))
                });
                tag.resolved
                    .sort_by(|a, b| a.normalized_text.cmp(&b.normalized_text));
                tag
            })
            .collect();
        tags.sort_by(|a, b| {
            b.sources
                .len()
                .cmp(&a.sources.len())
                .then_with(|| a.tag.cmp(&b.tag))
        });
        Ok(LiveObservations {
            tags,
            observations_read: reading.observations_read,
            decisions_read: reading.decisions_read,
        })
    }

    fn read(&self) -> Result<Reading> {
        let observations = self.observations.load_all()?;
        let prior_decisions = self.decisions.load_all()?;
        let decisions_read = prior_decisions.len();
        let mut closed: HashMap<(String, String), DecisionRecord> = HashMap::new();
        for decision in prior_decisions
            .into_iter()
            .filter(|d| d.decision.suppresses_resurfacing())
        {
            let key = (decision.tag.clone(), decision.normalized_text.clone());
            match closed.get(&key) {
                Some(standing) if standing.timestamp >= decision.timestamp => {}
                _ => {
                    closed.insert(key, decision);
                }
            }
        }

        let observations_read = observations.len();
        let mut groups: HashMap<(String, String), Vec<Observation>> = HashMap::new();
        for o in observations {
            groups
                .entry((o.tag.clone(), normalize(&o.text)))
                .or_default()
                .push(o);
        }

        let mut live = Vec::new();
        let mut resolved = Vec::new();
        for (key, items) in groups {
            if let Some(decision) = closed.get(&key) {
                resolved.push((
                    key.0,
                    ResolvedGroup {
                        normalized_text: key.1,
                        decision: decision.decision,
                    },
                ));
                continue;
            }
            let instance_count = items.len() as u32;
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
            let mut sources: Vec<String> = items.into_iter().map(|o| o.source).collect();
            sources.sort();
            sources.dedup();
            let (tag, normalized_text) = key;
            live.push(ObservationGroup {
                tag,
                normalized_text,
                instance_count,
                span_days: last.duration_since(first).as_secs() / 86400,
                first_seen: first,
                last_seen: last,
                sources,
            });
        }
        Ok(Reading {
            observations_read,
            decisions_read,
            live,
            resolved,
        })
    }
}

fn tag_entry<'m>(
    by_tag: &'m mut HashMap<String, TagObservations>,
    tag: &str,
) -> &'m mut TagObservations {
    by_tag
        .entry(tag.to_string())
        .or_insert_with(|| TagObservations {
            tag: tag.to_string(),
            sources: Vec::new(),
            groups: Vec::new(),
            resolved: Vec::new(),
        })
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
