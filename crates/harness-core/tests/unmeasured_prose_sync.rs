//! Drift guard for the reference passage that teaches by citing harnex's own
//! refusals.
//!
//! `reference/enforced-vs-advisory.md` tells the skill what a generated gate
//! owes when it cannot measure, and it teaches by naming the five surfaces
//! that already answer it. The skill generates from that passage, so a surface
//! that quietly loses its third answer does not merely leave the passage
//! wrong — it leaves the next harness designed against an enforcement nothing
//! performs.
//!
//! Each expectation is bound to the table ROW that makes the claim, not to the
//! document. Two rows answer `unmeasured` and a third contains `skipped`, so a
//! document-wide search is satisfied by a neighbour: the outcomes could be
//! swapped between rows, or a row reversed outright, with every predicate
//! still true.

use std::path::{Path, PathBuf};

use harness_core::config::{ConsumerDetectorDecl, LifecycleConfig};
use harness_core::guard::{StopAuditor, StopDecision};
use harness_core::lifecycle::{
    DecisionLedger, LedgerReader, ObservationLedger, RetirementClassifier, RetirementSignal,
    SilenceState, consumer_detector_for,
};
use harness_core::plan::AcceptanceCounts;

/// Each row of the passage's table: the surface it names, and the word that
/// row must answer with.
const REFUSALS: &[(&str, &str)] = &[
    ("lifecycle retire", "unmeasured"),
    ("plan audit", "unmeasured"),
    ("hooks/pre-commit", "unjudged"),
    ("lifecycle candidates", "observations_read"),
    ("guard stop-audit", "skip"),
];

/// A leak code has to survive the shell that carries it and stay clear of
/// every code already spoken for: 0 is a clean scan, 1 is how gitleaks itself
/// fails, 2 is shell misuse, 126 and 127 are an unexecutable or absent
/// command, and anything past 255 wraps — 256 arrives as 0, which turns a
/// finding into a clean scan. 128 and up are signals, and 130 is the one to
/// picture: the Ctrl-C an operator lands on a slow scan would report as
/// flagged secrets and steer them to the hatch that stops scanning for good.
const LEAK_CODE_RANGE: std::ops::RangeInclusive<i64> = 3..=125;

fn plugin_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{relative} unreadable at {}: {e}", path.display()))
}

/// The one table row whose first cell names `surface`.
fn row(passage: &str, surface: &str) -> String {
    let mut found = passage.lines().filter(|line| {
        line.starts_with('|')
            && line
                .split('|')
                .nth(1)
                .is_some_and(|cell| cell.contains(surface))
    });
    let row = found
        .next()
        .unwrap_or_else(|| panic!("the passage has no table row naming `{surface}`"))
        .to_string();
    assert!(
        found.next().is_none(),
        "`{surface}` names more than one row, so an expectation cannot bind to either"
    );
    row
}

#[test]
fn every_row_answers_with_the_word_this_guard_watches() {
    let passage = plugin_file("reference/enforced-vs-advisory.md");
    for (surface, word) in REFUSALS {
        let row = row(&passage, surface);
        let outcome = row
            .split('|')
            .nth(3)
            .unwrap_or_else(|| panic!("the `{surface}` row has no outcome cell: {row}"));
        assert!(
            outcome.contains(word),
            "the `{surface}` row must answer with `{word}`, and answers: {outcome}"
        );
    }
}

#[test]
fn the_table_carries_no_row_this_guard_does_not_watch() {
    // Binding each expectation to its row catches a row that changed; nothing
    // in that catches a row that arrived. A sixth surface added here ships an
    // uncited claim the skill would generate against, so the table's own size
    // is the denominator — the same reason `plugin_prose_sync` declares a
    // count per document.
    let passage = plugin_file("reference/enforced-vs-advisory.md");
    let section = passage
        .split("## Unmeasured is not passed")
        .nth(1)
        .expect("the passage carries the section")
        .split("\n## ")
        .next()
        .expect("bounded by the next heading");
    let body = section
        .lines()
        .filter(|line| line.starts_with('|'))
        .skip(2) // the header and its separator
        .count();
    assert_eq!(
        body,
        REFUSALS.len(),
        "the doctrine table holds {body} surfaces and this guard watches \
         {}; register the new row above, or drop it",
        REFUSALS.len()
    );
}

#[test]
fn retirement_fires_no_silence_signal_for_a_slug_it_could_not_measure() {
    let dir = tempfile::tempdir().unwrap();
    let rules = dir.path().join(".claude/rules");
    std::fs::create_dir_all(&rules).unwrap();
    let rule = rules.join("naming.md");
    std::fs::write(&rule, "body").unwrap();

    let config = LifecycleConfig {
        promotion_min_instances: 3,
        promotion_min_days: 0,
        stale_days: 30,
        silence_window_days: 90,
        grace_period_days: 0,
        observation_dir: dir.path().join("obs"),
        decision_dir: dir.path().join("dec"),
        consumer_detectors: vec![ConsumerDetectorDecl {
            kind: "rule".into(),
            strategy: "grep".into(),
            pattern: "{slug}".into(),
            exclude_globs: vec![],
        }],
    };
    let detector = consumer_detector_for(config.consumer_detectors[0].clone(), dir.path()).unwrap();
    let classifier = RetirementClassifier::new(&config, None);

    let unmeasured = classifier
        .classify("rule", &rule, detector.as_ref(), SilenceState::Unmeasured)
        .unwrap();
    assert!(
        !unmeasured.signals.contains(&RetirementSignal::Silent),
        "a silence that could not be measured must fire no signal: {:?}",
        unmeasured.signals
    );

    let silent = classifier
        .classify("rule", &rule, detector.as_ref(), SilenceState::Silent)
        .unwrap();
    assert!(
        silent.signals.contains(&RetirementSignal::Silent),
        "and a measured silence must still fire one, or this guard proves nothing"
    );
}

#[test]
fn an_acceptance_criterion_nothing_answered_blocks_as_a_blocker_does() {
    let unanswered = AcceptanceCounts {
        passed: 0,
        failed: 0,
        unmeasured: 1,
    };
    assert_eq!(unanswered.blocking(), 1);
    assert_eq!(
        AcceptanceCounts {
            passed: 1,
            failed: 0,
            unmeasured: 0,
        }
        .blocking(),
        0,
        "and a criterion that passed does not, or the count says nothing"
    );
}

#[test]
fn the_candidate_survey_reports_the_corpus_beside_the_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let config = LifecycleConfig {
        promotion_min_instances: 3,
        promotion_min_days: 0,
        stale_days: 30,
        silence_window_days: 90,
        grace_period_days: 0,
        observation_dir: dir.path().join("never-written"),
        decision_dir: dir.path().join("dec"),
        consumer_detectors: Vec::new(),
    };
    let observations = ObservationLedger::new(config.observation_dir.clone());
    let decisions = DecisionLedger::new(config.decision_dir.clone());
    let survey = LedgerReader::new(&config, &observations, &decisions)
        .survey()
        .unwrap();
    let carried = serde_json::to_value(&survey).unwrap();
    assert!(
        carried.get("observations_read").is_some(),
        "the survey stopped reporting the corpus it read: {carried}"
    );

    // And a ledger it could not read is an error rather than that same zero.
    let blocker = dir.path().join("not-a-directory");
    std::fs::write(&blocker, "").unwrap();
    let unreadable = ObservationLedger::new(blocker.join("obs"));
    assert!(
        LedgerReader::new(&config, &unreadable, &decisions)
            .survey()
            .is_err(),
        "a ledger that could not be read must fail, not report a corpus of zero"
    );
}

#[test]
fn the_stop_audit_skips_rather_than_deciding_when_its_probe_gave_no_answer() {
    let dir = tempfile::tempdir().unwrap();
    let config = harness_core::config::StopAuditConfig {
        runtime: "claude-code".into(),
        critique_skill: "/critique".into(),
        max_retries: 3,
        // The probe the loader refuses, which a hand-built config still holds.
        has_changes_check: Vec::new(),
        retry_ledger_dir: dir.path().join("_audit_retry"),
    };
    let decision = StopAuditor::new(&config, dir.path(), "sess".into())
        .run()
        .unwrap();
    assert!(
        matches!(decision, StopDecision::Skip { .. }),
        "an audit that cannot reach a verdict must skip, never decide: {decision:?}"
    );
    assert!(
        !Path::new(&config.retry_ledger_dir).exists(),
        "and it must spend nothing — not even the retry ledger"
    );
}

#[test]
fn the_secret_scan_gives_its_findings_a_code_the_shell_carries_intact() {
    let hook = plugin_file("templates/common/git-hooks/pre-commit");
    // The row credits the hook with a word, so the hook has to still say it —
    // otherwise the passage cites vocabulary the surface dropped, which is the
    // drift binding to the row moved one step rather than closed.
    let (_, word) = REFUSALS
        .iter()
        .find(|(surface, _)| *surface == "hooks/pre-commit")
        .expect("the pre-commit row is watched");
    assert!(
        hook.contains(word),
        "the passage credits the scan with `{word}` and the shipped hook no longer says it"
    );
    let raw = hook
        .lines()
        .find_map(|line| line.trim().strip_prefix("GITLEAKS_LEAK_CODE="))
        .expect("the shipped pre-commit declares a distinct code for findings");
    let code: i64 = raw
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("GITLEAKS_LEAK_CODE={raw} is not a number ({e})"));
    assert!(
        LEAK_CODE_RANGE.contains(&code),
        "GITLEAKS_LEAK_CODE={code} is outside {LEAK_CODE_RANGE:?} — outside it the code \
         either collides with one the scan or the shell already uses, or wraps past 255 \
         and arrives as a different one, and a finding stops being distinguishable from \
         a clean scan"
    );
}
