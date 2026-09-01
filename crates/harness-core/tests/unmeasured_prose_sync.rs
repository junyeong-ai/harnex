//! Drift guard for the reference passage that teaches by citing harnex's own
//! refusals.
//!
//! `reference/enforced-vs-advisory.md` tells the skill what a generated gate
//! owes when it cannot measure, and it teaches by naming the five surfaces
//! that already answer it. The skill generates from that passage, so a surface
//! that quietly loses its third answer does not merely leave the passage
//! wrong — it leaves the next harness designed against an enforcement nothing
//! performs. Both directions, as everywhere else here: a word the oracle
//! dropped while the passage still names it, and a surface this guard claims
//! to watch that the passage stopped citing.

use std::path::PathBuf;

use harness_core::guard::StopDecision;
use harness_core::lifecycle::SilenceState;
use harness_core::plan::AcceptanceCounts;

/// Each row of the passage's table: the surface it names, and the word that
/// surface answers with when it could not measure.
const REFUSALS: &[(&str, &str)] = &[
    ("lifecycle retire", "unmeasured"),
    ("plan audit", "unmeasured"),
    ("hooks/pre-commit", "unscanned"),
    ("lifecycle candidates", "observations_read"),
    ("guard stop-audit", "skip"),
];

fn plugin_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{relative} unreadable at {}: {e}", path.display()))
}

#[test]
fn the_passage_names_every_surface_and_the_word_it_answers_with() {
    let passage = plugin_file("reference/enforced-vs-advisory.md");
    for (surface, word) in REFUSALS {
        assert!(
            passage.contains(surface),
            "the passage stopped citing `{surface}`, which this guard watches"
        );
        assert!(
            passage.contains(word),
            "the passage cites `{surface}` without the `{word}` it answers with"
        );
    }
}

#[test]
fn every_surface_still_carries_the_answer_the_passage_credits_it_with() {
    // The oracle's side of each row, read from the type rather than restated.
    assert!(
        SilenceState::ALL
            .iter()
            .any(|state| state.as_str() == "unmeasured"),
        "retirement lost its unmeasured silence, and the passage still promises it"
    );

    // An acceptance criterion nothing answered blocks exactly as a Blocker
    // does — the claim is the arithmetic, not the field's presence.
    let unanswered = AcceptanceCounts {
        passed: 0,
        failed: 0,
        unmeasured: 1,
    };
    assert_eq!(
        unanswered.blocking(),
        1,
        "an unanswered criterion stopped blocking, and the passage still says it does"
    );

    let skipped = serde_json::to_value(StopDecision::Skip {
        reason: "the probe gave no answer".into(),
    })
    .expect("a decision serialises");
    assert_eq!(
        skipped["decision"], "skip",
        "the Stop audit lost the answer it gives when it cannot reach a verdict"
    );

    // The secret scan's findings need a code of their own, or a scan that
    // failed exits like one that found nothing.
    let hook = plugin_file("templates/common/git-hooks/pre-commit");
    let code = hook
        .lines()
        .find_map(|line| line.trim().strip_prefix("GITLEAKS_LEAK_CODE="))
        .expect("the shipped pre-commit declares a distinct code for findings");
    assert!(
        !matches!(code, "0" | "1" | "2" | "126" | "127"),
        "GITLEAKS_LEAK_CODE={code} collides with a code the scan or the shell \
         already uses, so a failed scan is indistinguishable from a clean one"
    );
}
