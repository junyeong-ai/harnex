//! # lifecycle — promotion / retirement / consumer detection
//!
//! Treats harness elements (rules, skills, hooks) as nodes with lifecycle.
//! Promotion surfaces candidates that crossed (instances × time) thresholds;
//! retirement classifies stale + unused + silent elements; consumer detection
//! finds every file referencing a slug (grep or graph-backlinks strategy);
//! the decision ledger records human-authored promote / reject / defer / demote
//! verdicts.
//!
//! [`RetirementSweeper`] is the top-level retirement runner: it walks every
//! `[[kinds]]` declaration, finds the matching consumer detector, classifies
//! each glob-matched file, and derives the `Silent` signal automatically
//! by scanning the telemetry ledger for slug string occurrences.
//!
//! ## What this module refuses to do
//!
//! - Never auto-promote / auto-retire. All transitions require explicit
//!   human-authored decision text via the [`LifecycleDecisionRecorder`] verbs.
//! - Never invent observation or decision text — callers supply both.
//! - Never silently delete ledger records on rotation.

pub mod consumer;
pub mod decision;
pub mod observation;
pub mod decision_recorder;
pub mod retire;
pub mod retirement;

pub use consumer::{
    ConsumerDetector, ConsumerStrategy, GraphBacklinksConsumerDetector, GrepConsumerDetector,
    consumer_detector_for,
};
pub use decision::{DecisionLedger, DecisionRecord, PromotionDecision};
pub use observation::{Observation, ObservationLedger};
pub use decision_recorder::{LifecycleDecisionRecorder, PromotionCandidate, PromotionCandidateFinder};
pub use retire::{RetirementSweeper, SweepOutcome};
pub use retirement::{RetirementClassifier, RetirementSignal, RetirementOutcome};
