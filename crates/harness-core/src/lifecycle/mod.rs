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

use crate::error::{Error, Result};

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
    CandidateSurvey, LifecycleDecisionRecorder, PromotionCandidate, PromotionCandidateFinder,
};
pub use observation::{Observation, ObservationLedger};
pub use retire::{RetirementSweeper, SweepOutcome};
pub use retirement::{RetirementClassifier, RetirementOutcome, RetirementSignal, SilenceState};

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
///
/// An empty tag is refused here rather than encoded, because it is the one
/// input whose stem yields a filename the readers cannot see: `.jsonl` is a
/// dotfile with no extension, so it passes every write and matches no scan.
/// This is the boundary — both ledgers append through it, so what is written
/// is what can be read back.
pub(crate) fn tag_filename_stem(tag: &str) -> Result<String> {
    if tag.trim().is_empty() {
        return Err(Error::LifecycleTagEmpty);
    }
    let mut out = String::with_capacity(tag.len());
    for b in tag.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tag_filename_tests {
    use super::tag_filename_stem;
    use crate::error::{Error, Result};

    fn stem(tag: &str) -> String {
        tag_filename_stem(tag).unwrap()
    }

    #[test]
    fn path_separators_are_encoded() {
        assert_eq!(stem("rust/async"), "rust%2Fasync");
        assert!(!stem("a/b/c").contains('/'));
        assert!(!stem("a\\b").contains('\\'));
    }

    #[test]
    fn dots_pass_through_safely() {
        // `.` is safe: the caller appends `.jsonl`, so a `..` tag yields the
        // stem `..` → filename `...jsonl`, a single ordinary filename, never
        // a traversal component.
        assert_eq!(stem(".."), "..");
        assert_eq!(stem("v1.2"), "v1.2");
    }

    #[test]
    fn safe_tags_pass_through_and_are_injective() {
        assert_eq!(stem("error-handling"), "error-handling");
        // `%` itself is encoded, so a literal tag cannot collide with an
        // encoded one.
        assert_ne!(stem("a/b"), stem("a%2Fb"));
    }

    #[test]
    fn every_accepted_tag_yields_a_filename_the_scan_matches() {
        // The stem is only safe if the readers can still see the file. Both
        // ledgers scan for a `jsonl` extension, so a stem that leaves the
        // filename extensionless writes a record nothing reads back.
        for tag in ["naming", "rust/async", "..", "v1.2", "a%2Fb", "  padded  "] {
            let name = format!("{}.jsonl", stem(tag));
            assert_eq!(
                std::path::Path::new(&name)
                    .extension()
                    .and_then(|e| e.to_str()),
                Some("jsonl"),
                "tag {tag:?} files as {name}, which the ledger scan skips"
            );
        }
    }

    #[test]
    fn an_empty_tag_is_refused_rather_than_filed_where_nothing_reads_it() {
        for tag in ["", "   ", "\t\n"] {
            assert!(
                matches!(
                    tag_filename_stem(tag) as Result<String>,
                    Err(Error::LifecycleTagEmpty)
                ),
                "tag {tag:?} must be refused: its stem files as `.jsonl`, which every scan skips"
            );
        }
    }
}
