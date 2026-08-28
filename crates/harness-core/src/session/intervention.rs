//! # intervention — where the operator stepped into a turn the agent was taking
//!
//! The question this answers is what the operator did *during* a turn rather
//! than between turns, which is the part of a session no summary of the work
//! recovers. Two acts are recorded structurally and both are here; a third is
//! deliberately absent.
//!
//! [`InterventionKind::Steering`] is exact. A queued turn arriving after the
//! agent has produced output is the operator not waiting, and the runtime
//! writes down both halves of that. [`InterventionKind::MarkedInterrupt`] is a
//! floor — the runtime records the marker on some interruptions and not
//! others, so the count is a lower bound and the type says so where a consumer
//! reads it.
//!
//! ## What this module refuses to do
//!
//! - Never treat a refused tool call as an intervention. `user-rejected` reads
//!   like the operator saying no, and over the local corpus it is four
//!   different events sharing one wire value: a compound command whose
//!   sub-command needed approval (143), an expansion guard (38), a single
//!   command needing approval (37), and the operator actually refusing (32).
//!   The four are separable only in the message text. Counting all of them
//!   would report interventions at thirteen times their observed number.
//! - Never infer an interruption from wording. The marker is a floor and is
//!   published as one; the alternative reports zero the day the wording moves.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::session::record::{Authorship, Citation, UserTurn};
use crate::wire_enum::wire_enum;

wire_enum! {
    /// An act the operator took inside a turn the agent was still taking.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum InterventionKind {
        /// The operator sent an instruction while the agent was working, after it
        /// had already produced output.
        Steering => "steering",
        /// The runtime recorded an agent turn as cut short. A floor: over the
        /// local corpus the marker is present on 216 of 394 interruptions (54.8%).
        MarkedInterrupt => "marked-interrupt",
    }
}

/// One act, precise enough to reopen and read around.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Intervention {
    pub kind: String,
    pub citation: Citation,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InterventionFacts {
    /// Oldest first, so a reader follows the session rather than the ranking.
    pub interventions: Vec<Intervention>,
    /// Counts keyed by [`InterventionKind::as_str`].
    pub by_kind: BTreeMap<String, usize>,
}

#[derive(Default)]
pub struct InterventionAnalyzer {
    interventions: Vec<Intervention>,
}

impl InterventionAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, turn: &UserTurn) {
        // The marker rides a record the runtime wrote, not one the operator
        // typed, so it is read before authorship rather than under it.
        if turn.interrupted {
            self.push(InterventionKind::MarkedInterrupt, turn);
        }
        if turn.authorship == Authorship::Authored && turn.queued && turn.follows_agent_output {
            self.push(InterventionKind::Steering, turn);
        }
    }

    fn push(&mut self, kind: InterventionKind, turn: &UserTurn) {
        self.interventions.push(Intervention {
            kind: kind.as_str().to_string(),
            citation: turn.citation.clone(),
        });
    }

    pub fn finish(mut self) -> InterventionFacts {
        self.interventions
            .sort_by_key(|i| (i.citation.timestamp, i.kind.clone()));
        let mut by_kind = BTreeMap::new();
        for kind in InterventionKind::ALL {
            by_kind.insert(kind.as_str().to_string(), 0);
        }
        for i in &self.interventions {
            *by_kind.entry(i.kind.clone()).or_default() += 1;
        }
        InterventionFacts {
            interventions: self.interventions,
            by_kind,
        }
    }
}

#[cfg(test)]
mod kind_tests {
    use super::InterventionKind;

    #[test]
    fn from_str_round_trips_every_variant() {
        for k in InterventionKind::ALL {
            assert_eq!(InterventionKind::from_str(k.as_str()), Some(*k));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert_eq!(InterventionKind::from_str("user-rejected"), None);
    }
}
