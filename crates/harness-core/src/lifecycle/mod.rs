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
pub mod decision_recorder;
pub mod observation;
pub mod retire;
pub mod retirement;

pub use consumer::{
    ConsumerDetector, ConsumerStrategy, GraphBacklinksConsumerDetector, GrepConsumerDetector,
    consumer_detector_for,
};
pub use decision::{DecisionLedger, DecisionRecord, PromotionDecision};
pub use decision_recorder::{
    LifecycleDecisionRecorder, PromotionCandidate, PromotionCandidateFinder,
};
pub use observation::{Observation, ObservationLedger};
pub use retire::{RetirementSweeper, SweepOutcome};
pub use retirement::{RetirementClassifier, RetirementOutcome, RetirementSignal};

/// Encode a tag into a filesystem-safe ledger filename stem. A tag is a
/// semantic grouping key (it may be namespaced, e.g. `rust/async`); the
/// real tag is always stored in the JSONL record body, so the filename
/// only needs to be safe and deterministic. Any character outside
/// `[A-Za-z0-9._-]` is percent-encoded — injective (so distinct tags never
/// collide into one ledger) and free of path separators (`/` → `%2F`,
/// `\` → `%5C`). Without this, a `/` in a tag would write into a
/// subdirectory the flat ledger reader never scans, silently losing the
/// observation from promotion candidates. `.` is left intact: the caller
/// always appends a `.jsonl` suffix, so the stem can never form a bare
/// `.`/`..` path component.
pub(crate) fn tag_filename_stem(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    for b in tag.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tag_filename_tests {
    use super::tag_filename_stem;

    #[test]
    fn path_separators_are_encoded() {
        assert_eq!(tag_filename_stem("rust/async"), "rust%2Fasync");
        assert!(!tag_filename_stem("a/b/c").contains('/'));
        assert!(!tag_filename_stem("a\\b").contains('\\'));
    }

    #[test]
    fn dots_pass_through_safely() {
        // `.` is safe: the caller appends `.jsonl`, so a `..` tag yields the
        // stem `..` → filename `...jsonl`, a single ordinary filename, never
        // a traversal component.
        assert_eq!(tag_filename_stem(".."), "..");
        assert_eq!(tag_filename_stem("v1.2"), "v1.2");
    }

    #[test]
    fn safe_tags_pass_through_and_are_injective() {
        assert_eq!(tag_filename_stem("error-handling"), "error-handling");
        // `%` itself is encoded, so a literal tag cannot collide with an
        // encoded one.
        assert_ne!(tag_filename_stem("a/b"), tag_filename_stem("a%2Fb"));
    }
}
