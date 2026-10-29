//! Integration tests for the lifecycle module.

use harness_core::config::{Config, TelemetryConfig, TelemetryKindDecl};
use harness_core::config::{
    ConsumerDetectorDecl, KindDecl, LifecycleConfig, RetirementConfig, RetirementExemptDecl,
};
use harness_core::error::Error;
use harness_core::lifecycle::{
    ConsumerDetector, DecisionLedger, LastChange, LedgerReader, LifecycleDecisionRecorder,
    ObservationLedger, PromotionDecision, RetirementClassifier, RetirementSignal,
    RetirementSweeper, SilenceState, last_changes,
};
use harness_core::telemetry::{Event, JsonlStorage, TelemetryAppender, TelemetryQuery};
use jiff::{SignedDuration, Timestamp};
use std::path::PathBuf;
use tempfile::TempDir;

mod common;

/// A temporary project that is a repository, which is what the grep consumer
/// detector reads: it asks git which files the project owns.
fn repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let init = common::git(tmp.path())
        .args(["init", "-q"])
        .status()
        .unwrap();
    assert!(init.success());
    tmp
}

fn default_lifecycle(observation_dir: PathBuf) -> LifecycleConfig {
    let parent = observation_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    LifecycleConfig {
        promotion_min_instances: 3,
        promotion_min_days: 0, // tests run within seconds
        stale_days: 30,
        silence_window_days: 90,
        grace_period_days: 0,
        observation_dir,
        decision_dir: parent.join("decisions"),
        consumer_detectors: vec![ConsumerDetectorDecl {
            kind: "rule".into(),
            pattern: "{slug}".into(),
            exclude_globs: vec![],
        }],
    }
}

fn decisions_for(tmp: &TempDir) -> DecisionLedger {
    DecisionLedger::new(tmp.path().join("decisions"))
}

fn seed_three_observations(ledger: &ObservationLedger) {
    for source in ["spec-a", "spec-b", "spec-c"] {
        ledger.append("naming", "use snake case", source).unwrap();
    }
}

/// Test-only convenience: bundles the read-only Reader and the write
/// Recorder into one struct, since most lifecycle tests need both.
struct TestPromoter<'a> {
    reader: LedgerReader<'a>,
    recorder: LifecycleDecisionRecorder<'a>,
}

fn mk_promoter<'a>(
    cfg: &'a LifecycleConfig,
    observations: &'a ObservationLedger,
    decisions: &'a DecisionLedger,
) -> TestPromoter<'a> {
    TestPromoter {
        reader: LedgerReader::new(cfg, observations, decisions),
        recorder: LifecycleDecisionRecorder::new(decisions),
    }
}

#[test]
fn observation_append_load_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    ledger.append("naming", "use snake_case", "spec-x").unwrap();
    ledger.append("naming", "use snake_case", "spec-y").unwrap();
    let all = ledger.load_all().unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn observation_with_namespaced_tag_round_trips() {
    // A tag containing a path separator (namespaced, e.g. `rust/async`) must
    // not write into a subdirectory the flat ledger reader never scans. The
    // filename is encoded; the real tag is preserved in the record body and
    // loads back intact.
    let tmp = TempDir::new().unwrap();
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    ledger
        .append("rust/async", "prefer tokio::spawn", "spec-a")
        .unwrap();
    ledger
        .append("rust/async", "prefer tokio::spawn", "spec-b")
        .unwrap();
    let all = ledger.load_all().unwrap();
    assert_eq!(all.len(), 2, "namespaced-tag observations must not be lost");
    assert!(all.iter().all(|o| o.tag == "rust/async"));
    // The slash never created a real subdirectory under the ledger dir.
    assert!(!tmp.path().join("rust").exists());
}

#[test]
fn promoter_lists_threshold_crossing_groups() {
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    seed_three_observations(&ledger);
    ledger
        .append("naming", "different observation", "spec-d")
        .unwrap();

    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    let candidates = promoter.reader.survey().unwrap().candidates;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].instance_count, 3);
    assert_eq!(candidates[0].normalized_text, "use snake case");
    assert_eq!(candidates[0].sources.len(), 3);
}

#[test]
fn promoter_excludes_below_threshold() {
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    ledger.append("naming", "only once", "spec-a").unwrap();
    ledger.append("naming", "only once", "spec-b").unwrap();
    let candidates = mk_promoter(&cfg, &ledger, &decisions)
        .reader
        .survey()
        .unwrap()
        .candidates;
    assert!(candidates.is_empty());
}

#[test]
fn promoter_normalizes_whitespace_and_case() {
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    ledger.append("x", "Use Snake Case", "a").unwrap();
    ledger.append("x", "use   snake case", "b").unwrap();
    ledger.append("x", "USE SNAKE CASE", "c").unwrap();
    let candidates = mk_promoter(&cfg, &ledger, &decisions)
        .reader
        .survey()
        .unwrap()
        .candidates;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].instance_count, 3);
}

#[test]
fn survey_tells_an_unwritten_ledger_from_a_corpus_that_produced_nothing() {
    // Three states that all yield no candidates, and the curate pass acts on
    // each differently: a ledger nobody has written to is a loop whose emit
    // half never fired, observations under the bar are the bar working, and a
    // resolved group is a decision already taken. A bare list is the same
    // answer to all three — a zero the pass never measured, read as a clean
    // corpus.
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().join("never-written"));
    let decisions = decisions_for(&tmp);

    let absent = ObservationLedger::new(tmp.path().join("never-written"));
    let survey = mk_promoter(&cfg, &absent, &decisions)
        .reader
        .survey()
        .unwrap();
    assert_eq!(survey.observations_read, 0);
    assert_eq!(survey.groups_considered, 0);
    assert_eq!(survey.groups_resolved, 0);
    let live = mk_promoter(&cfg, &absent, &decisions)
        .reader
        .live()
        .unwrap();
    assert!(live.tags.is_empty());
    assert_eq!(live.observations_read, 0);

    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    ledger.append("naming", "seen twice", "spec-a").unwrap();
    ledger.append("naming", "seen twice", "spec-b").unwrap();
    let survey = mk_promoter(&cfg, &ledger, &decisions)
        .reader
        .survey()
        .unwrap();
    assert!(survey.candidates.is_empty());
    assert_eq!(survey.observations_read, 2);
    assert_eq!(survey.groups_considered, 1);

    seed_three_observations(&ledger);
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    promoter
        .recorder
        .reject("naming", "use snake case", "the linter already enforces it")
        .unwrap();
    let survey = promoter.reader.survey().unwrap();
    assert!(survey.candidates.is_empty());
    assert_eq!(survey.observations_read, 5);
    assert_eq!(survey.groups_resolved, 1);
    // The resolved group left the considered set rather than the ledger: the
    // observations behind it are still read and still counted.
    assert_eq!(survey.groups_considered, 1);
}

#[test]
fn a_ledger_that_could_not_be_read_is_not_a_ledger_nobody_wrote_to() {
    // Both ledgers answer an absent directory with an empty result, and the
    // two reasons a directory can fail to resolve are opposite answers: never
    // written is a corpus of zero, unreadable is a corpus this pass did not
    // read. Reproduced with a regular file where a directory belongs, which
    // fails to stat for a reason that is not absence.
    let tmp = TempDir::new().unwrap();
    let blocker = tmp.path().join("not-a-directory");
    std::fs::write(&blocker, "").unwrap();

    let ledger = ObservationLedger::new(blocker.join("observations"));
    assert!(
        matches!(ledger.load_all(), Err(Error::IoFailure { .. })),
        "an unresolvable observation ledger must fail, not read as empty"
    );
    let decisions = DecisionLedger::new(blocker.join("decisions"));
    assert!(
        matches!(decisions.load_all(), Err(Error::IoFailure { .. })),
        "an unresolvable decision ledger must fail, not read as no decisions"
    );

    // And the survey carries it up rather than answering with counts it did
    // not measure.
    let cfg = default_lifecycle(blocker.join("observations"));
    assert!(
        matches!(
            mk_promoter(&cfg, &ledger, &decisions).reader.survey(),
            Err(Error::IoFailure { .. })
        ),
        "the survey must fail with its ledger, never report a fabricated zero"
    );
    assert!(
        matches!(
            mk_promoter(&cfg, &ledger, &decisions).reader.live(),
            Err(Error::IoFailure { .. })
        ),
        "and so must the live layout, which is the same reading"
    );
}

#[cfg(unix)]
#[test]
fn a_ledger_whose_symlink_lost_its_target_is_not_an_absent_ledger() {
    // `path_guard` sanctions a symlinked ancestor, so a ledger reached through
    // one is a supported layout — and when its target moves the path resolves
    // to nothing, which reads as "never written" while the records are
    // elsewhere. On the decision side that silently resurfaces every candidate
    // the operator settled, and a curate pass never writes, so nothing else in
    // the loop would notice.
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("real");
    let link = tmp.path().join("linked");
    std::fs::create_dir_all(&target).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let ledger = ObservationLedger::new(link.clone());
    ledger.append("naming", "use snake case", "spec-a").unwrap();
    assert_eq!(ledger.load_all().unwrap().len(), 1);

    std::fs::rename(&target, tmp.path().join("moved")).unwrap();
    assert!(
        matches!(ledger.load_all(), Err(Error::IoFailure { .. })),
        "a ledger symlink with no target must fail, not read as never written"
    );
    assert!(
        matches!(
            DecisionLedger::new(link).load_all(),
            Err(Error::IoFailure { .. })
        ),
        "the decision ledger owes the same answer, or settled candidates resurface"
    );

    // The genuinely absent directory is still an empty result, not an error.
    assert!(
        ObservationLedger::new(tmp.path().join("never-written"))
            .load_all()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_record_the_ledger_accepted_is_a_record_it_reads_back() {
    // Both ledgers file by tag and scan for a `jsonl` extension, so a tag
    // whose stem leaves the filename extensionless is written, reported
    // successful, and never seen again. The refusal is at the write, and it
    // covers both ledgers because both append through the same encoder.
    let tmp = TempDir::new().unwrap();
    let ledger = ObservationLedger::new(tmp.path().join("observations"));
    let decisions = DecisionLedger::new(tmp.path().join("decisions"));
    let recorder = LifecycleDecisionRecorder::new(&decisions);

    for tag in ["", "   "] {
        assert!(
            matches!(
                ledger.append(tag, "a real constraint", "spec-a"),
                Err(Error::LifecycleTagEmpty)
            ),
            "an observation under tag {tag:?} must be refused, not filed unreadably"
        );
        assert!(
            matches!(
                recorder.reject(tag, "a real constraint", "no"),
                Err(Error::LifecycleTagEmpty)
            ),
            "a decision under tag {tag:?} must be refused, or the suppression set loses it"
        );
    }

    // Nothing was written by a refused append, and every tag the ledgers do
    // accept survives the round trip.
    for tag in ["naming", "rust/async", "..", "v1.2", " padded "] {
        ledger.append(tag, "a real constraint", "spec-a").unwrap();
        recorder.reject(tag, "a real constraint", "no").unwrap();
    }
    assert_eq!(ledger.load_all().unwrap().len(), 5);
    assert_eq!(decisions.load_all().unwrap().len(), 5);
}

#[test]
fn the_scan_reads_a_record_filed_under_a_leading_dot_name() {
    // `Path::extension` answers `None` for `.jsonl`, so a scan written against
    // it skips a whole ledger file. Nothing writes that name now, and a record
    // an older build filed under it is still a record — a read that comes back
    // short is what both ledgers refuse everywhere else.
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("observations");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(".jsonl"),
        "{\"tag\":\"\",\"text\":\"filed by an older build\",\"source\":\"spec-z\",\
         \"timestamp\":\"2026-08-01T00:00:00Z\"}\n",
    )
    .unwrap();

    let ledger = ObservationLedger::new(dir);
    ledger.append("naming", "use snake case", "spec-a").unwrap();
    assert_eq!(ledger.load_all().unwrap().len(), 2);
}

#[test]
fn survey_reports_the_decision_ledger_it_read_too() {
    // `groups_resolved: 0` is the same number for a pass that has closed
    // nothing and for a decision ledger that was not found — and the second
    // silently resurfaces every candidate the operator already rejected.
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    seed_three_observations(&ledger);

    let recorded = decisions_for(&tmp);
    let promoter = mk_promoter(&cfg, &ledger, &recorded);
    promoter
        .recorder
        .reject("naming", "use snake case", "the linter already enforces it")
        .unwrap();
    let honoured = promoter.reader.survey().unwrap();
    assert_eq!(honoured.groups_resolved, 1);
    assert_eq!(honoured.decisions_read, 1);

    let elsewhere = DecisionLedger::new(tmp.path().join("relocated"));
    let lost = mk_promoter(&cfg, &ledger, &elsewhere)
        .reader
        .survey()
        .unwrap();
    assert_eq!(lost.groups_resolved, 0, "the rejection is out of reach");
    assert_eq!(
        lost.decisions_read, 0,
        "and the survey says so rather than reporting a settled corpus"
    );
}

#[test]
fn survey_accounts_for_every_group_the_ledger_holds() {
    // The counts close over the whole ledger, so a candidate list shorter
    // than expected is explained by the survey itself rather than read as
    // records the grouper lost.
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    seed_three_observations(&ledger);
    for source in ["spec-a", "spec-b"] {
        ledger
            .append("errors", "wrap at the boundary", source)
            .unwrap();
    }
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    promoter
        .recorder
        .promote("errors", "wrap at the boundary", "landed in errors.md")
        .unwrap();

    let survey = promoter.reader.survey().unwrap();
    assert_eq!(survey.observations_read, 5);
    assert_eq!(survey.groups_considered + survey.groups_resolved, 2);
    assert!(survey.candidates.len() <= survey.groups_considered);
}

#[test]
fn survey_orders_tied_candidates_the_same_way_every_run() {
    // Groups arrive from a hash map, so equal instance counts would otherwise
    // order differently run to run over an unchanged ledger (Article I).
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    for tag in ["naming", "errors", "logging"] {
        for source in ["spec-a", "spec-b", "spec-c"] {
            ledger.append(tag, "same weight", source).unwrap();
        }
    }
    let ordering: Vec<String> = mk_promoter(&cfg, &ledger, &decisions)
        .reader
        .survey()
        .unwrap()
        .candidates
        .into_iter()
        .map(|c| c.tag)
        .collect();
    assert_eq!(ordering, ["errors", "logging", "naming"]);
}

#[test]
fn live_lays_out_every_open_wording_by_tag_widest_breadth_first() {
    // The thresholds count recurrence per exact wording, so one claim in two
    // wordings never crosses them; the live layout is where a reader sees
    // both under one tag. A tag orders by how many sources observed it, a
    // wording under it by how many times.
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    seed_three_observations(&ledger);
    ledger
        .append("naming", "prefer snake_case identifiers", "spec-d")
        .unwrap();
    for source in ["spec-a", "spec-b"] {
        ledger
            .append("errors", "wrap at the boundary", source)
            .unwrap();
    }
    for _ in 0..5 {
        ledger
            .append("logging", "one line per event", "spec-a")
            .unwrap();
    }
    ledger
        .append("logging", "no secrets in log lines", "spec-a")
        .unwrap();

    let live = mk_promoter(&cfg, &ledger, &decisions)
        .reader
        .live()
        .unwrap();
    assert_eq!(live.observations_read, 12);
    let breadth: Vec<(&str, usize)> = live
        .tags
        .iter()
        .map(|t| (t.tag.as_str(), t.sources.len()))
        .collect();
    assert_eq!(
        breadth,
        [("naming", 4), ("errors", 2), ("logging", 1)],
        "six sightings of two wordings from one source are less breadth than two from two"
    );
    let naming = &live.tags[0];
    assert_eq!(naming.sources, ["spec-a", "spec-b", "spec-c", "spec-d"]);
    let wordings: Vec<(&str, u32)> = naming
        .groups
        .iter()
        .map(|g| (g.normalized_text.as_str(), g.instance_count))
        .collect();
    assert_eq!(
        wordings,
        [("use snake case", 3), ("prefer snake_case identifiers", 1)]
    );
    assert_eq!(live.tags[2].groups[0].instance_count, 5);
}

#[test]
fn live_excludes_what_a_decision_closed_and_counts_it_under_its_tag() {
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    seed_three_observations(&ledger);
    ledger
        .append("naming", "prefer snake_case identifiers", "spec-d")
        .unwrap();
    for source in ["spec-a", "spec-b"] {
        ledger
            .append("errors", "wrap at the boundary", source)
            .unwrap();
    }
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    promoter
        .recorder
        .defer("naming", "use snake case", "revisit after spec-z")
        .unwrap();
    let deferred = promoter.reader.live().unwrap();
    assert_eq!(
        deferred.tags[0].groups.len(),
        2,
        "a deferral keeps a wording open"
    );
    assert!(deferred.tags[0].resolved.is_empty());

    promoter
        .recorder
        .reject("naming", "use snake case", "the linter already enforces it")
        .unwrap();
    promoter
        .recorder
        .promote("errors", "wrap at the boundary", "landed in errors.md")
        .unwrap();
    let live = promoter.reader.live().unwrap();
    assert_eq!(live.decisions_read, 3);
    let naming = &live.tags[0];
    assert_eq!(naming.tag, "naming");
    assert_eq!(naming.groups.len(), 1);
    assert_eq!(
        naming.groups[0].normalized_text,
        "prefer snake_case identifiers"
    );
    let closed: Vec<(&str, PromotionDecision)> = naming
        .resolved
        .iter()
        .map(|r| (r.normalized_text.as_str(), r.decision))
        .collect();
    assert_eq!(closed, [("use snake case", PromotionDecision::Rejected)]);
    assert_eq!(
        naming.sources,
        ["spec-d"],
        "a closed wording's sources are no longer the tag's breadth"
    );
    // A tag with nothing left open is still listed, with the wording and the
    // decision a new sighting under it is checked against.
    let errors = &live.tags[1];
    assert_eq!(errors.tag, "errors");
    assert!(errors.groups.is_empty());
    assert!(errors.sources.is_empty());
    assert_eq!(errors.resolved.len(), 1);
    assert_eq!(errors.resolved[0].decision, PromotionDecision::Approved);

    // A wording closed twice shows the latest closing decision, and a later
    // deferral does not reopen or relabel it.
    promoter
        .recorder
        .demote("errors", "wrap at the boundary", "the rule misfired")
        .unwrap();
    promoter
        .recorder
        .defer("naming", "use snake case", "revisit after spec-z")
        .unwrap();
    let live = promoter.reader.live().unwrap();
    assert_eq!(
        live.tags[1].resolved[0].decision,
        PromotionDecision::Demoted
    );
    assert_eq!(
        live.tags[0].resolved[0].decision,
        PromotionDecision::Rejected
    );
    assert_eq!(live.tags[0].groups.len(), 1);

    // Closed wordings list by wording, so an author checks theirs by eye.
    promoter
        .recorder
        .reject(
            "naming",
            "prefer snake_case identifiers",
            "the linter already enforces it",
        )
        .unwrap();
    let live = promoter.reader.live().unwrap();
    let closed: Vec<&str> = live
        .tags
        .iter()
        .find(|t| t.tag == "naming")
        .expect("listed with nothing open")
        .resolved
        .iter()
        .map(|r| r.normalized_text.as_str())
        .collect();
    assert_eq!(closed, ["prefer snake_case identifiers", "use snake case"]);
}

#[test]
fn live_and_survey_are_one_reading_of_the_ledger() {
    // Both views project one reading, so their counts close against each
    // other: every group is open in one and considered in the other, or
    // resolved in both, and a candidate is an open group that crossed.
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    seed_three_observations(&ledger);
    ledger
        .append("naming", "prefer snake_case identifiers", "spec-d")
        .unwrap();
    for source in ["spec-a", "spec-b"] {
        ledger
            .append("errors", "wrap at the boundary", source)
            .unwrap();
    }
    ledger
        .append("logging", "one line per event", "spec-a")
        .unwrap();
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    promoter
        .recorder
        .promote("errors", "wrap at the boundary", "landed in errors.md")
        .unwrap();

    let survey = promoter.reader.survey().unwrap();
    let live = promoter.reader.live().unwrap();
    assert_eq!(live.observations_read, survey.observations_read);
    assert_eq!(live.decisions_read, survey.decisions_read);
    let open: usize = live.tags.iter().map(|t| t.groups.len()).sum();
    let closed: usize = live.tags.iter().map(|t| t.resolved.len()).sum();
    assert_eq!(open, survey.groups_considered);
    assert_eq!(closed, survey.groups_resolved);
    assert_eq!(survey.candidates.len(), 1);
    for candidate in &survey.candidates {
        let tag = live
            .tags
            .iter()
            .find(|t| t.tag == candidate.tag)
            .expect("a candidate's tag is open");
        let group = tag
            .groups
            .iter()
            .find(|g| g.normalized_text == candidate.normalized_text)
            .expect("a candidate is an open group");
        assert_eq!(group.instance_count, candidate.instance_count);
        assert_eq!(group.sources, candidate.sources);
    }
}

#[test]
fn the_day_threshold_is_met_by_a_span_of_that_many_days_and_not_one_second_less() {
    // Every other test runs with `promotion_min_days: 0`, so this is the one
    // place the day threshold is measured against a span at all; the records
    // are written by hand because the ledger stamps the clock.
    let tmp = TempDir::new().unwrap();
    let mut cfg = default_lifecycle(tmp.path().to_path_buf());
    cfg.promotion_min_days = 30;
    let start = Timestamp::from_second(1_800_000_000).unwrap();
    let record = |text: &str, source: &str, at: Timestamp| {
        serde_json::json!({
            "tag": "span",
            "text": text,
            "source": source,
            "timestamp": at,
        })
        .to_string()
    };
    let thirty = SignedDuration::from_hours(30 * 24);
    let lines = [
        record("exactly thirty", "spec-a", start),
        record("exactly thirty", "spec-b", start + thirty / 2),
        record("exactly thirty", "spec-c", start + thirty),
        record("one second short", "spec-a", start),
        record("one second short", "spec-b", start + thirty / 2),
        record(
            "one second short",
            "spec-c",
            start + thirty - SignedDuration::from_secs(1),
        ),
    ];
    std::fs::write(tmp.path().join("span.jsonl"), lines.join("\n")).unwrap();

    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    let survey = mk_promoter(&cfg, &ledger, &decisions)
        .reader
        .survey()
        .unwrap();
    let crossed: Vec<(&str, i64)> = survey
        .candidates
        .iter()
        .map(|c| (c.normalized_text.as_str(), c.span_days))
        .collect();
    assert_eq!(crossed, [("exactly thirty", 30)]);
    assert_eq!(survey.groups_considered, 2);
}

#[test]
fn live_orders_ties_the_same_way_every_run() {
    // Tags of one breadth and wordings of one count would otherwise order
    // differently run to run over an unchanged ledger (Article I).
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    for tag in ["naming", "errors", "logging"] {
        ledger.append(tag, "zeta wording", "spec-a").unwrap();
        ledger.append(tag, "alpha wording", "spec-a").unwrap();
    }
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    for wording in ["zeta closed", "alpha closed", "mid closed"] {
        ledger.append("archive", wording, "spec-a").unwrap();
        promoter
            .recorder
            .reject("archive", wording, "settled")
            .unwrap();
    }
    let live = promoter.reader.live().unwrap();
    let tags: Vec<&str> = live.tags.iter().map(|t| t.tag.as_str()).collect();
    assert_eq!(
        tags,
        ["errors", "logging", "naming", "archive"],
        "breadth before name: a tag with nothing open sorts last whatever it is called"
    );
    for tag in &live.tags[..3] {
        let wordings: Vec<&str> = tag
            .groups
            .iter()
            .map(|g| g.normalized_text.as_str())
            .collect();
        assert_eq!(wordings, ["alpha wording", "zeta wording"]);
    }
    let closed: Vec<&str> = live.tags[3]
        .resolved
        .iter()
        .map(|r| r.normalized_text.as_str())
        .collect();
    assert_eq!(closed, ["alpha closed", "mid closed", "zeta closed"]);
}

#[test]
fn consumer_detector_finds_references() {
    // The rule names itself in its own body, so the exclude is what keeps an
    // artifact from being its own consumer.
    let tmp = repo();
    let rule_dir = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rule_dir).unwrap();
    std::fs::write(rule_dir.join("my-rule.md"), "# my-rule\n\nrule body").unwrap();
    let consumer1 = tmp.path().join("docs/spec.md");
    std::fs::create_dir_all(consumer1.parent().unwrap()).unwrap();
    std::fs::write(&consumer1, "see my-rule for details").unwrap();
    let unrelated = tmp.path().join("docs/other.md");
    std::fs::write(&unrelated, "irrelevant content").unwrap();

    let detector = ConsumerDetector::new(
        ConsumerDetectorDecl {
            kind: "rule".into(),
            pattern: "{slug}".into(),
            exclude_globs: vec![".claude/rules/{slug}.md".into()],
        },
        tmp.path(),
    )
    .unwrap();
    let consumers = detector.find_consumers("my-rule").unwrap();
    assert_eq!(
        consumers,
        [PathBuf::from("docs/spec.md")],
        "the excluded rule file is not its own consumer"
    );
}

#[test]
fn consumer_detector_reads_what_the_project_owns_and_nothing_else_on_the_machine() {
    // The set is the project's own: tracked, plus untracked the project's
    // `.gitignore` does not ignore. A match in build output is not a consumer
    // — and a walk would have counted it, differently on every machine.
    let tmp = repo();
    std::fs::write(tmp.path().join(".gitignore"), "target/\n").unwrap();
    let real = tmp.path().join("docs/spec.md");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, "see my-rule").unwrap();
    let status = common::git(tmp.path())
        .args(["add", ".gitignore", "docs/spec.md"])
        .status()
        .unwrap();
    assert!(status.success());
    let target = tmp.path().join("target/debug");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("build.log"), "references my-rule here").unwrap();
    std::fs::write(tmp.path().join("notes.md"), "my-rule, untracked as yet").unwrap();

    let detector = ConsumerDetector::new(
        ConsumerDetectorDecl {
            kind: "rule".into(),
            pattern: "{slug}".into(),
            exclude_globs: vec![],
        },
        tmp.path(),
    )
    .unwrap();
    let mut consumers = detector.find_consumers("my-rule").unwrap();
    consumers.sort();
    assert_eq!(
        consumers,
        [PathBuf::from("docs/spec.md"), PathBuf::from("notes.md")],
        "the ignored build output is not a consumer and the author's untracked file is"
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_is_its_link_text_and_neither_its_target_nor_a_second_count() {
    // git owns the link text, not what it points at: a target outside the
    // project is the machine's content, and a target inside it is already
    // counted under its own path.
    let tmp = repo();
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("notes.md"), "mentions my-rule").unwrap();
    let real = tmp.path().join("docs/spec.md");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, "see my-rule").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("notes.md"),
        tmp.path().join("elsewhere.md"),
    )
    .unwrap();
    std::os::unix::fs::symlink("docs/spec.md", tmp.path().join("again.md")).unwrap();

    let detector = ConsumerDetector::new(
        ConsumerDetectorDecl {
            kind: "rule".into(),
            pattern: "{slug}".into(),
            exclude_globs: vec![],
        },
        tmp.path(),
    )
    .unwrap();
    assert_eq!(
        detector.find_consumers("my-rule").unwrap(),
        [PathBuf::from("docs/spec.md")]
    );
}

#[test]
fn an_unmerged_file_is_one_consumer_not_one_per_index_stage() {
    // A conflicted path sits in the index once per stage, and a listing that
    // repeats it counts one reader three times.
    let tmp = repo();
    let git = |args: &[&str]| {
        let status = common::git(tmp.path())
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    std::fs::write(tmp.path().join("conf.md"), "base my-rule\n").unwrap();
    git(&["add", "conf.md"]);
    git(&["commit", "-q", "-m", "base"]);
    git(&["checkout", "-q", "-b", "other"]);
    std::fs::write(tmp.path().join("conf.md"), "theirs my-rule\n").unwrap();
    git(&["commit", "-q", "-am", "theirs"]);
    git(&["checkout", "-q", "-"]);
    std::fs::write(tmp.path().join("conf.md"), "ours my-rule\n").unwrap();
    git(&["commit", "-q", "-am", "ours"]);
    let merge = common::git(tmp.path())
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(["merge", "-q", "other"])
        .status()
        .unwrap();
    assert!(
        !merge.success(),
        "the merge must conflict for this to test anything"
    );
    let unmerged = common::git(tmp.path())
        .args(["ls-files", "-u"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&unmerged.stdout).lines().count(),
        3,
        "and the conflict must leave the path in three index stages, or the \
         listing has nothing to repeat"
    );

    let detector = ConsumerDetector::new(
        ConsumerDetectorDecl {
            kind: "rule".into(),
            pattern: "{slug}".into(),
            exclude_globs: vec![],
        },
        tmp.path(),
    )
    .unwrap();
    assert_eq!(
        detector.find_consumers("my-rule").unwrap(),
        [PathBuf::from("conf.md")]
    );
}

#[test]
fn a_directory_that_is_not_a_repository_is_refused_rather_than_walked() {
    let tmp = TempDir::new().unwrap();
    let real = tmp.path().join("docs/spec.md");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, "see my-rule").unwrap();
    let decl = ConsumerDetectorDecl {
        kind: "rule".into(),
        pattern: "{slug}".into(),
        exclude_globs: vec![],
    };
    assert!(
        matches!(
            ConsumerDetector::new(decl.clone(), tmp.path()),
            Err(Error::LifecycleGitFailure { .. })
        ),
        "without git the set the detector reads does not exist, and a walk in its place \
         would be the machine's answer, not the project's"
    );

    // The sweep does not die with the kind: it declares the kind unmeasured
    // and sweeps the rest, as `check` declares nested memory unmeasured.
    let rules = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("naming.md"), "a rule").unwrap();
    let mut cfg = build_sweep_config(
        tmp.path(),
        vec![KindDecl {
            name: "rule".into(),
            glob: ".claude/rules/*.md".into(),
            foundation: false,
            invocation_kind: None,
        }],
    );
    cfg.lifecycle.as_mut().unwrap().consumer_detectors = vec![decl];
    let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
    let query = TelemetryQuery::new(storage);
    let outcome = RetirementSweeper::new(&cfg, tmp.path(), &query)
        .unwrap()
        .run()
        .unwrap();
    let skip = outcome
        .kinds_skipped
        .iter()
        .find(|s| s.slug == "rule")
        .expect("the kind is declared, not dropped");
    assert!(
        skip.reason.starts_with("consumers unmeasured:"),
        "the reason names what could not be read: {}",
        skip.reason
    );
    assert!(outcome.verdicts.is_empty());
    assert!(outcome.kinds_processed.is_empty());
}

#[test]
fn retirement_classifier_marks_no_consumers_silent() {
    let tmp = repo();
    let rule_dir = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rule_dir).unwrap();
    let rule_path = rule_dir.join("orphan-rule.md");
    std::fs::write(&rule_path, "body").unwrap();
    let cfg = default_lifecycle(tmp.path().join(".harness/observations"));
    let detector = ConsumerDetector::new(cfg.consumer_detectors[0].clone(), tmp.path()).unwrap();
    let classifier = RetirementClassifier::new(&cfg, None);
    let verdict = classifier
        .classify(
            "rule",
            &rule_path,
            &detector,
            SilenceState::Silent,
            LastChange::Uncommitted,
        )
        .unwrap();
    assert!(verdict.signals.contains(&RetirementSignal::NoConsumers));
    assert!(verdict.signals.contains(&RetirementSignal::Silent));
}

#[test]
fn retirement_classifier_honors_exempt_list() {
    let tmp = repo();
    let rule_dir = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rule_dir).unwrap();
    let rule_path = rule_dir.join("constitution.md");
    std::fs::write(&rule_path, "body").unwrap();
    let cfg = default_lifecycle(tmp.path().join(".harness/observations"));
    let retire_cfg = RetirementConfig {
        exempt: RetirementExemptDecl {
            kinds: vec![],
            slugs: vec!["constitution".into()],
        },
    };
    let detector = ConsumerDetector::new(cfg.consumer_detectors[0].clone(), tmp.path()).unwrap();
    let classifier = RetirementClassifier::new(&cfg, Some(&retire_cfg));
    let verdict = classifier
        .classify(
            "rule",
            &rule_path,
            &detector,
            SilenceState::Silent,
            LastChange::Uncommitted,
        )
        .unwrap();
    assert!(verdict.exempt);
}

#[test]
fn an_artifact_no_history_records_is_neither_stale_nor_new() {
    // No history here dates it, so its age is unknown: not zero, not old.
    let tmp = repo();
    let rule_dir = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rule_dir).unwrap();
    let rule_path = rule_dir.join("shared.md");
    std::fs::write(&rule_path, "body").unwrap();
    let mut cfg = default_lifecycle(tmp.path().join(".harness/observations"));
    cfg.grace_period_days = 30;
    let detector = ConsumerDetector::new(cfg.consumer_detectors[0].clone(), tmp.path()).unwrap();
    let verdict = RetirementClassifier::new(&cfg, None)
        .classify(
            "rule",
            &rule_path,
            &detector,
            SilenceState::Silent,
            LastChange::Unrecorded,
        )
        .unwrap();
    assert_eq!(verdict.age_days, None);
    assert!(!verdict.signals.contains(&RetirementSignal::Stale));
    assert!(!verdict.exempt, "no recorded change puts it in its grace");
}

#[test]
fn promote_rejects_empty_decision_text() {
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    let err = promoter
        .recorder
        .promote("naming", "use snake case", "   ")
        .unwrap_err();
    assert_eq!(
        err.code(),
        harness_core::error::ErrorCode::LifecycleDecisionTextEmpty
    );
}

#[test]
fn reject_and_defer_share_text_validation() {
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    assert_eq!(
        promoter.recorder.reject("t", "x", "").unwrap_err().code(),
        harness_core::error::ErrorCode::LifecycleDecisionTextEmpty
    );
    assert_eq!(
        promoter.recorder.defer("t", "x", "").unwrap_err().code(),
        harness_core::error::ErrorCode::LifecycleDecisionTextEmpty
    );
}

#[test]
fn approve_excludes_from_future_listing() {
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    seed_three_observations(&ledger);
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    assert_eq!(promoter.reader.survey().unwrap().candidates.len(), 1);
    promoter
        .recorder
        .promote(
            "naming",
            "use snake case",
            "promoted to naming-conventions.md",
        )
        .unwrap();
    assert!(promoter.reader.survey().unwrap().candidates.is_empty());
}

#[test]
fn reject_also_excludes() {
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    seed_three_observations(&ledger);
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    promoter
        .recorder
        .reject("naming", "use snake case", "team owns naming per-package")
        .unwrap();
    assert!(promoter.reader.survey().unwrap().candidates.is_empty());
}

#[test]
fn defer_keeps_surfacing() {
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    seed_three_observations(&ledger);
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    promoter
        .recorder
        .defer("naming", "use snake case", "revisit after spec-z")
        .unwrap();
    assert_eq!(promoter.reader.survey().unwrap().candidates.len(), 1);
}

#[test]
fn demote_refused_without_prior_approval() {
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    let err = promoter
        .recorder
        .demote("naming", "use snake case", "rationale")
        .unwrap_err();
    assert_eq!(
        err.code(),
        harness_core::error::ErrorCode::LifecycleDemoteWithoutApproval
    );
}

#[test]
fn promote_after_demote_is_allowed_and_resuppresses() {
    // State machine: Approved → Demoted → Approved is legitimate
    // "rehabilitation" — operator may re-promote after demotion. Each
    // decision is an append-only record; surfacing is suppressed as long
    // as ANY suppressing decision exists in the ledger. Re-promoting
    // simply appends a new Approved record on top of the prior Demoted.
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    seed_three_observations(&ledger);
    let promoter = mk_promoter(&cfg, &ledger, &decisions);

    promoter
        .recorder
        .promote("naming", "use snake case", "v1")
        .unwrap();
    promoter
        .recorder
        .demote("naming", "use snake case", "rolled back")
        .unwrap();
    let re_promote = promoter
        .recorder
        .promote("naming", "use snake case", "v2 after rehab")
        .unwrap();
    assert_eq!(re_promote.decision, PromotionDecision::Approved);

    // Ledger holds all three decisions in append-only history
    let all = decisions.load_all().unwrap();
    let naming_decisions: Vec<_> = all
        .iter()
        .filter(|d| d.tag == "naming")
        .map(|d| d.decision)
        .collect();
    assert_eq!(naming_decisions.len(), 3);
    // Surfacing remains suppressed regardless of which decision is latest
    assert!(promoter.reader.survey().unwrap().candidates.is_empty());
}

#[test]
fn second_demote_without_re_approval_is_refused() {
    // State machine: after Approved → Demoted, the LATEST state is Demoted.
    // A second demote without an intervening Approved must be refused —
    // there's no "approved state" to retract from.
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    let promoter = mk_promoter(&cfg, &ledger, &decisions);

    promoter
        .recorder
        .promote("naming", "use snake case", "v1")
        .unwrap();
    promoter
        .recorder
        .demote("naming", "use snake case", "rolled back")
        .unwrap();
    // Second demote with no intervening Approved must fail
    let err = promoter
        .recorder
        .demote("naming", "use snake case", "trying again")
        .unwrap_err();
    assert_eq!(
        err.code(),
        harness_core::error::ErrorCode::LifecycleDemoteWithoutApproval
    );
}

#[test]
fn demote_after_rehab_re_approval_is_allowed() {
    // After Approved → Demoted → re-Approved, latest is Approved.
    // A demote should succeed (operator can re-retract).
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    let promoter = mk_promoter(&cfg, &ledger, &decisions);

    promoter.recorder.promote("naming", "v", "v1").unwrap();
    promoter.recorder.demote("naming", "v", "rollback").unwrap();
    promoter
        .recorder
        .promote("naming", "v", "v2 rehab")
        .unwrap();
    // Now latest is Approved again — demote should succeed
    let again = promoter
        .recorder
        .demote("naming", "v", "second rollback")
        .unwrap();
    assert_eq!(again.decision, PromotionDecision::Demoted);
}

#[test]
fn demote_succeeds_after_approval_and_excludes_from_listing() {
    let tmp = TempDir::new().unwrap();
    let cfg = default_lifecycle(tmp.path().to_path_buf());
    let ledger = ObservationLedger::new(tmp.path().to_path_buf());
    let decisions = decisions_for(&tmp);
    seed_three_observations(&ledger);
    let promoter = mk_promoter(&cfg, &ledger, &decisions);
    promoter
        .recorder
        .promote("naming", "use snake case", "promoted v1")
        .unwrap();
    let demoted = promoter
        .recorder
        .demote(
            "naming",
            "use snake case",
            "rule proved narrow; rolled back",
        )
        .unwrap();
    assert_eq!(demoted.decision, PromotionDecision::Demoted);
    // Both Approved AND Demoted live in the ledger; both suppress surfacing.
    assert!(promoter.reader.survey().unwrap().candidates.is_empty());
}

#[test]
fn suppression_predicate_matches_documented_set() {
    assert!(PromotionDecision::Approved.suppresses_resurfacing());
    assert!(PromotionDecision::Rejected.suppresses_resurfacing());
    assert!(PromotionDecision::Demoted.suppresses_resurfacing());
    assert!(!PromotionDecision::Deferred.suppresses_resurfacing());
}

// ---------- DecisionLedger filtering ----------

#[test]
fn decision_ledger_round_trips_all_four_decisions() {
    let tmp = TempDir::new().unwrap();
    let ledger = DecisionLedger::new(tmp.path().to_path_buf());
    let cfg = default_lifecycle(tmp.path().join("obs"));
    let obs = ObservationLedger::new(tmp.path().join("obs"));
    let promoter = mk_promoter(&cfg, &obs, &ledger);

    promoter.recorder.promote("t1", "x", "promoted").unwrap();
    promoter.recorder.reject("t2", "y", "rejected").unwrap();
    promoter.recorder.defer("t3", "z", "deferred").unwrap();
    // Need prior approval for demote
    promoter
        .recorder
        .promote("t4", "w", "approved-first")
        .unwrap();
    promoter.recorder.demote("t4", "w", "then-demoted").unwrap();

    let records = ledger.load_all().unwrap();
    assert_eq!(records.len(), 5);

    let approved: Vec<_> = records
        .iter()
        .filter(|r| r.decision == PromotionDecision::Approved)
        .collect();
    assert_eq!(approved.len(), 2);
    let rejected: Vec<_> = records
        .iter()
        .filter(|r| r.decision == PromotionDecision::Rejected)
        .collect();
    assert_eq!(rejected.len(), 1);
    let deferred: Vec<_> = records
        .iter()
        .filter(|r| r.decision == PromotionDecision::Deferred)
        .collect();
    assert_eq!(deferred.len(), 1);
    let demoted: Vec<_> = records
        .iter()
        .filter(|r| r.decision == PromotionDecision::Demoted)
        .collect();
    assert_eq!(demoted.len(), 1);
}

// ---------- RetirementSweeper ----------

fn telemetry_cfg(dir: PathBuf) -> TelemetryConfig {
    TelemetryConfig {
        storage: "jsonl".into(),
        storage_dir: dir,
        rotate_at_mb: 10,
        kinds: vec![TelemetryKindDecl {
            name: "skill-invoked".into(),
            payload_schema: serde_json::json!({
                "type": "object",
                "required": ["skill", "outcome"],
                "properties": {
                    "skill": {"type": "string"},
                    "outcome": {"type": "string", "enum": ["ok", "warn", "fail"]}
                }
            }),
        }],
    }
}

fn build_sweep_config(tmp_path: &std::path::Path, extra_kinds: Vec<KindDecl>) -> Config {
    let cfg = harness_core::config::Config {
        meta: harness_core::config::MetaConfig {
            harnex_version: common::FIXTURE_PIN.into(),
        },
        kinds: extra_kinds,
        evidence: None,
        telemetry: Some(telemetry_cfg(tmp_path.join("tele"))),
        policy: None,
        validate: None,
        lifecycle: Some(default_lifecycle(tmp_path.join("obs"))),
        retirement: None,
        guard: None,
        session: None,
    };
    cfg.validate().unwrap();
    cfg
}

#[test]
fn sweep_walks_every_kind_with_consumer_detector() {
    let tmp = repo();
    let rule_dir = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rule_dir).unwrap();
    std::fs::write(
        rule_dir.join("rule-a.md"),
        "---\npaths: [\"x\"]\n---\nbody\n",
    )
    .unwrap();
    std::fs::write(
        rule_dir.join("rule-b.md"),
        "---\npaths: [\"y\"]\n---\nbody\n",
    )
    .unwrap();

    let cfg = build_sweep_config(
        tmp.path(),
        vec![KindDecl {
            name: "rule".into(),
            glob: ".claude/rules/*.md".into(),
            foundation: false,
            invocation_kind: Some("skill-invoked".into()),
        }],
    );
    let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
    let query = TelemetryQuery::new(storage);

    let sweep = RetirementSweeper::new(&cfg, tmp.path(), &query).unwrap();
    let outcome = sweep.run().unwrap();
    assert_eq!(outcome.files_classified, 2);
    assert!(outcome.kinds_processed.contains(&"rule".to_string()));
    // No telemetry ledger exists, so silence is Unmeasured — never a fabricated
    // Silent. NoConsumers fires; the telemetry-derived signal does not.
    for v in &outcome.verdicts {
        assert!(v.signals.contains(&RetirementSignal::NoConsumers));
        assert_eq!(v.silence, SilenceState::Unmeasured);
        assert!(!v.signals.contains(&RetirementSignal::Silent));
    }
}

/// Commit everything in `dir`, dated `days` ago by both clocks git records.
fn commit_days_ago(dir: &std::path::Path, days: i64) {
    let at = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(24 * days);
    let date = format!("{} +0000", at.as_second());
    for args in [
        &["add", "-A"][..],
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "x",
        ],
    ] {
        let status = common::git(dir)
            .args(args)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }
}

#[test]
fn the_sweep_dates_each_artifact_by_its_history_not_its_checkout() {
    // Every file here was written moments ago, as a fresh clone writes them;
    // only the history says how long each has gone unchanged.
    let tmp = repo();
    let rules = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("old.md"), "a rule").unwrap();
    std::fs::write(rules.join("edited.md"), "a rule").unwrap();
    commit_days_ago(tmp.path(), 200);
    std::fs::write(rules.join("recent.md"), "a rule").unwrap();
    commit_days_ago(tmp.path(), 3);
    std::fs::write(rules.join("edited.md"), "a rule, mid-edit").unwrap();
    std::fs::write(rules.join("new.md"), "a rule").unwrap();

    let cfg = build_sweep_config(
        tmp.path(),
        vec![KindDecl {
            name: "rule".into(),
            glob: ".claude/rules/*.md".into(),
            foundation: false,
            invocation_kind: None,
        }],
    );
    let query = TelemetryQuery::new(JsonlStorage::new(tmp.path().join("tele"), 10));
    let outcome = RetirementSweeper::new(&cfg, tmp.path(), &query)
        .unwrap()
        .run()
        .unwrap();
    let verdict = |slug: &str| {
        let v = outcome
            .verdicts
            .iter()
            .find(|v| v.slug == slug)
            .unwrap_or_else(|| panic!("no verdict for {slug}"));
        (v.age_days, v.signals.contains(&RetirementSignal::Stale))
    };
    let (age, stale) = verdict("old");
    assert!(matches!(age, Some(199..=200)) && stale, "old: {age:?} days");
    let (age, stale) = verdict("recent");
    assert!(matches!(age, Some(2..=3)) && !stale, "recent: {age:?} days");
    assert_eq!(
        verdict("edited"),
        (Some(0), false),
        "an uncommitted edit is a change now"
    );
    assert_eq!(
        verdict("new"),
        (Some(0), false),
        "a file never committed is new"
    );
}

#[test]
fn a_shallow_clone_is_refused_rather_than_dated_by_its_boundary() {
    let origin = repo();
    std::fs::write(origin.path().join("old.md"), "a rule").unwrap();
    commit_days_ago(origin.path(), 200);
    std::fs::write(origin.path().join("recent.md"), "a rule").unwrap();
    commit_days_ago(origin.path(), 1);
    let tmp = TempDir::new().unwrap();
    let url = format!("file://{}", origin.path().display());
    let cloned = common::git(tmp.path())
        .args(["clone", "-q", "--depth", "1", &url, "clone"])
        .status()
        .unwrap();
    assert!(cloned.success());
    let clone = tmp.path().join("clone");

    let err = last_changes(&clone, &[clone.join("old.md")]).unwrap_err();
    assert_eq!(
        err.code(),
        harness_core::error::ErrorCode::LifecycleGitFailure
    );
    assert!(err.to_string().contains("shallow"), "{err}");

    let full = last_changes(origin.path(), &[origin.path().join("old.md")]).unwrap();
    assert!(
        matches!(full[..], [LastChange::Committed(_)]),
        "the same file in the full history is dated: {full:?}"
    );
}

fn days_since(change: &LastChange) -> Option<i64> {
    match change {
        LastChange::Committed(at) => Some(Timestamp::now().duration_since(*at).as_secs() / 86_400),
        LastChange::Uncommitted | LastChange::Unrecorded => None,
    }
}

#[test]
fn a_directory_is_dated_by_the_files_it_holds() {
    // A skill is the directory its files sit in, and git lists only files.
    let tmp = repo();
    let skills = tmp.path().join(".claude/skills");
    for name in ["settled", "littered", "edited", "grown"] {
        std::fs::create_dir_all(skills.join(name)).unwrap();
        std::fs::write(skills.join(name).join("SKILL.md"), "a skill").unwrap();
    }
    commit_days_ago(tmp.path(), 200);
    std::fs::write(tmp.path().join(".git/info/exclude"), ".DS_Store\n").unwrap();
    std::fs::write(skills.join("littered/.DS_Store"), "finder").unwrap();
    std::fs::write(skills.join("edited/SKILL.md"), "a skill, mid-edit").unwrap();
    std::fs::write(skills.join("grown/reference.md"), "not yet committed").unwrap();

    let changes = last_changes(
        tmp.path(),
        &["settled", "littered", "edited", "grown"].map(|name| skills.join(name)),
    )
    .unwrap();
    let ages: Vec<_> = changes.iter().map(days_since).collect();
    assert!(
        matches!(ages[..2], [Some(199..=200), Some(199..=200)])
            && matches!(
                changes[2..],
                [LastChange::Uncommitted, LastChange::Uncommitted]
            ),
        "an ignored file is no change; an edited file and an untracked one are: {changes:?}"
    );
}

#[cfg(unix)]
#[test]
fn what_no_history_here_records_has_no_age() {
    // A skill linked in from elsewhere, a personal rule the project ignores,
    // and one inside a repository of its own are files this history never
    // records, however long they have sat unchanged.
    let tmp = repo();
    let outside = TempDir::new().unwrap();
    std::fs::create_dir_all(outside.path().join("alpha")).unwrap();
    std::fs::write(outside.path().join("alpha/SKILL.md"), "a skill").unwrap();
    let rules = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("rule.md"), "a rule").unwrap();
    std::fs::write(rules.join("personal.md"), "mine").unwrap();
    std::fs::write(tmp.path().join(".gitignore"), "personal.md\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), tmp.path().join(".claude/skills")).unwrap();
    let vendored = tmp.path().join("vendor/rules");
    std::fs::create_dir_all(&vendored).unwrap();
    std::fs::write(vendored.join("shared.md"), "a shared rule").unwrap();
    let nested = common::git(&vendored)
        .args(["init", "-q"])
        .status()
        .unwrap();
    assert!(nested.success());
    commit_days_ago(&vendored, 200);
    commit_days_ago(tmp.path(), 200);

    let changes = last_changes(
        tmp.path(),
        &[
            rules.join("rule.md"),
            tmp.path().join(".claude/skills/alpha"),
            rules.join("personal.md"),
            vendored.join("shared.md"),
            rules.join("missing.md"),
        ],
    )
    .unwrap();
    assert!(
        matches!(
            changes[..],
            [
                LastChange::Committed(_),
                LastChange::Unrecorded,
                LastChange::Unrecorded,
                LastChange::Unrecorded,
                LastChange::Unrecorded
            ]
        ),
        "{changes:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_project_in_a_subdirectory_reads_the_history_of_its_whole_repository() {
    // A monorepo package links in skills its repository keeps beside it.
    let tmp = repo();
    let shared = tmp.path().join("shared/skills");
    for name in ["settled", "edited"] {
        std::fs::create_dir_all(shared.join(name)).unwrap();
        std::fs::write(shared.join(name).join("SKILL.md"), "a skill").unwrap();
    }
    let app = tmp.path().join("app");
    std::fs::create_dir_all(app.join(".claude")).unwrap();
    std::os::unix::fs::symlink("../../shared/skills", app.join(".claude/skills")).unwrap();
    commit_days_ago(tmp.path(), 200);
    std::fs::write(shared.join("edited/SKILL.md"), "a skill, mid-edit").unwrap();

    let changes = last_changes(
        &app,
        &["settled", "edited"].map(|name| app.join(".claude/skills").join(name)),
    )
    .unwrap();
    assert!(
        matches!(days_since(&changes[0]), Some(199..=200)) && changes[1] == LastChange::Uncommitted,
        "{changes:?}"
    );
}

#[test]
fn a_repository_with_no_commits_holds_pending_changes_and_unrecorded_files() {
    let tmp = repo();
    std::fs::write(tmp.path().join(".gitignore"), "ignored.md\n").unwrap();
    for name in ["ignored.md", "new.md", "staged.md"] {
        std::fs::write(tmp.path().join(name), name).unwrap();
    }
    let staged = common::git(tmp.path())
        .args(["add", "staged.md"])
        .status()
        .unwrap();
    assert!(staged.success());

    let changes = last_changes(
        tmp.path(),
        &["ignored.md", "new.md", "staged.md"].map(|name| tmp.path().join(name)),
    )
    .unwrap();
    assert_eq!(
        changes,
        [
            LastChange::Unrecorded,
            LastChange::Uncommitted,
            LastChange::Uncommitted
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_path_that_cannot_be_resolved_fails_the_measurement() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = repo();
    let locked = tmp.path().join("locked");
    std::fs::create_dir_all(locked.join("inner")).unwrap();
    std::fs::write(locked.join("inner/rule.md"), "a rule").unwrap();
    commit_days_ago(tmp.path(), 200);
    let mode = |bits| std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(bits));
    mode(0o000).unwrap();
    // A superuser reads through any mode, and then nothing here is refused.
    let refused = std::fs::read_dir(&locked).is_err();
    let result = last_changes(tmp.path(), &[locked.join("inner/rule.md")]);
    mode(0o755).unwrap();
    if refused {
        let err = result.unwrap_err();
        assert_eq!(
            err.code(),
            harness_core::error::ErrorCode::LifecycleGitFailure
        );
    }
}

#[test]
fn a_name_is_dated_however_its_characters_are_composed() {
    // macOS hands back a Korean name decomposed. git lists it composed where
    // `core.precomposeUnicode` is on, its macOS default, and as stored where
    // it is off, as on every other system.
    let decomposed = "\u{1112}\u{1161}\u{11AB}\u{1100}\u{1173}\u{11AF}.md";
    let composed = "\u{D55C}\u{AE00}.md";
    for precompose in ["true", "false"] {
        let tmp = repo();
        let set = common::git(tmp.path())
            .args(["config", "core.precomposeUnicode", precompose])
            .status()
            .unwrap();
        assert!(set.success());
        std::fs::write(tmp.path().join(decomposed), "a rule").unwrap();
        commit_days_ago(tmp.path(), 200);

        // A file system that composes names finds the file either way.
        let spellings: Vec<_> = [decomposed, composed]
            .map(|name| tmp.path().join(name))
            .into_iter()
            .filter(|path| path.exists())
            .collect();
        let changes = last_changes(tmp.path(), &spellings).unwrap();
        for (spelling, change) in spellings.iter().zip(&changes) {
            assert!(
                matches!(days_since(change), Some(199..=200)),
                "precomposeUnicode={precompose}, {}: {change:?}",
                spelling.display()
            );
        }
    }
}

#[test]
fn a_move_changes_the_directory_it_left() {
    let tmp = repo();
    let skills = tmp.path().join(".claude/skills");
    for name in ["left", "reached", "leaving", "reaching"] {
        std::fs::create_dir_all(skills.join(name)).unwrap();
        std::fs::write(skills.join(name).join("SKILL.md"), name).unwrap();
    }
    std::fs::write(skills.join("left/reference.md"), "moved").unwrap();
    std::fs::write(skills.join("leaving/reference.md"), "moving").unwrap();
    commit_days_ago(tmp.path(), 200);
    let mv = |from: &str, to: &str| {
        let moved = common::git(tmp.path())
            .args(["mv", from, to])
            .status()
            .unwrap();
        assert!(moved.success(), "git mv {from} {to}");
    };
    mv(
        ".claude/skills/left/reference.md",
        ".claude/skills/reached/reference.md",
    );
    commit_days_ago(tmp.path(), 1);
    mv(
        ".claude/skills/leaving/reference.md",
        ".claude/skills/reaching/reference.md",
    );

    let changes = last_changes(
        tmp.path(),
        &["left", "reached", "leaving", "reaching"].map(|name| skills.join(name)),
    )
    .unwrap();
    let ages: Vec<_> = changes.iter().map(days_since).collect();
    assert!(
        matches!(ages[..2], [Some(0..=1), Some(0..=1)])
            && matches!(
                changes[2..],
                [LastChange::Uncommitted, LastChange::Uncommitted]
            ),
        "{changes:?}"
    );
}

#[test]
fn a_merge_dates_what_it_brought_to_the_branch() {
    // A side branch wrote a rule long ago and changed another the trunk had
    // also changed; the merge landed both yesterday, resolving the conflict.
    let tmp = repo();
    let git = |args: &[&str], days: i64| {
        let at = Timestamp::now() - SignedDuration::from_hours(24 * days);
        let date = format!("{} +0000", at.as_second());
        common::git(tmp.path())
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .status()
            .unwrap()
    };
    std::fs::write(tmp.path().join("both.md"), "a rule").unwrap();
    commit_days_ago(tmp.path(), 300);
    assert!(git(&["switch", "-q", "-c", "side"], 0).success());
    std::fs::write(tmp.path().join("both.md"), "the side's rule").unwrap();
    std::fs::write(tmp.path().join("side.md"), "a rule").unwrap();
    commit_days_ago(tmp.path(), 200);
    assert!(git(&["switch", "-q", "-"], 0).success());
    std::fs::write(tmp.path().join("both.md"), "the trunk's rule").unwrap();
    commit_days_ago(tmp.path(), 150);
    assert!(!git(&["merge", "-q", "--no-ff", "side"], 1).success());
    std::fs::write(tmp.path().join("both.md"), "the resolved rule").unwrap();
    commit_days_ago(tmp.path(), 1);

    let changes = last_changes(
        tmp.path(),
        &[tmp.path().join("side.md"), tmp.path().join("both.md")],
    )
    .unwrap();
    let ages: Vec<_> = changes.iter().map(days_since).collect();
    assert!(matches!(ages[..], [Some(0..=1), Some(0..=1)]), "{ages:?}");
}

#[test]
fn the_history_reads_the_same_under_any_log_configuration() {
    // A developer's configuration signs every commit, prints each signature's
    // verification into `git log`, and hides what the first commit added.
    let tmp = repo();
    let keys = TempDir::new().unwrap();
    let key = keys.path().join("key");
    let generated = std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "t@t", "-f"])
        .arg(&key)
        .status()
        .unwrap();
    assert!(generated.success());
    let public = std::fs::read_to_string(key.with_extension("pub")).unwrap();
    let allowed = keys.path().join("allowed");
    std::fs::write(&allowed, format!("t@t {public}")).unwrap();
    for (name, value) in [
        ("gpg.format", "ssh"),
        ("user.signingKey", key.to_str().unwrap()),
        ("gpg.ssh.allowedSignersFile", allowed.to_str().unwrap()),
        ("commit.gpgSign", "true"),
        ("log.showSignature", "true"),
        ("log.showRoot", "false"),
    ] {
        let set = common::git(tmp.path())
            .args(["config", name, value])
            .status()
            .unwrap();
        assert!(set.success(), "git config {name}");
    }
    std::fs::write(tmp.path().join("old.md"), "a rule").unwrap();
    commit_days_ago(tmp.path(), 200);
    std::fs::write(tmp.path().join("recent.md"), "a rule").unwrap();
    commit_days_ago(tmp.path(), 3);

    let changes = last_changes(
        tmp.path(),
        &[tmp.path().join("old.md"), tmp.path().join("recent.md")],
    )
    .unwrap();
    let ages: Vec<_> = changes.iter().map(days_since).collect();
    assert!(
        matches!(ages[..], [Some(199..=200), Some(2..=3)]),
        "{ages:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_path_is_dated_however_it_is_spelled() {
    let tmp = repo();
    let rules = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("old.md"), "a rule").unwrap();
    commit_days_ago(tmp.path(), 200);
    let links = TempDir::new().unwrap();
    std::os::unix::fs::symlink(tmp.path(), links.path().join("project")).unwrap();

    let mut spellings = vec![
        rules.join("old.md"),
        tmp.path().join("./.claude/rules/old.md"),
        tmp.path().join(".claude/../.claude/rules/old.md"),
        links.path().join("project/.claude/rules/old.md"),
    ];
    // A file system that folds case finds the file under any case of its name.
    let folded = tmp.path().join(".CLAUDE/RULES/OLD.md");
    if folded.exists() {
        spellings.push(folded);
    }
    let changes = last_changes(tmp.path(), &spellings).unwrap();
    for (spelling, change) in spellings.iter().zip(&changes) {
        assert!(
            matches!(days_since(change), Some(199..=200)),
            "{}: {change:?}",
            spelling.display()
        );
    }
}

#[test]
fn a_name_whose_case_alone_changed_keeps_its_history_where_git_ignores_case() {
    // Finder renames `Old.md` to `old.md` without git: `git status` stays
    // clean, since `core.ignorecase` reads the file as the one it tracks.
    let tmp = repo();
    let rules = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("Old.md"), "a rule").unwrap();
    commit_days_ago(tmp.path(), 200);
    std::fs::rename(rules.join("Old.md"), rules.join("renaming.md")).unwrap();
    std::fs::rename(rules.join("renaming.md"), rules.join("old.md")).unwrap();
    let ignorecase = common::git(tmp.path())
        .args(["config", "--type=bool", "core.ignorecase"])
        .output()
        .unwrap();
    if String::from_utf8_lossy(&ignorecase.stdout).trim() != "true" {
        return;
    }

    let spellings = ["old.md", "Old.md", "OLD.md"].map(|name| rules.join(name));
    let changes = last_changes(tmp.path(), &spellings).unwrap();
    for (spelling, change) in spellings.iter().zip(&changes) {
        assert!(
            matches!(days_since(change), Some(199..=200)),
            "{}: {change:?}",
            spelling.display()
        );
    }
}

#[cfg(unix)]
#[test]
fn a_linked_artifact_changes_when_its_link_or_what_it_reads_does() {
    // Skills a project links in: one from beside it whose files were edited
    // after the link was made, one newly linked to old files, one from
    // outside the repository, and one linked a moment ago.
    let tmp = repo();
    let tools = tmp.path().join("tools/skills");
    for name in ["edited", "old"] {
        std::fs::create_dir_all(tools.join(name)).unwrap();
        std::fs::write(tools.join(name).join("SKILL.md"), "a skill").unwrap();
    }
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("SKILL.md"), "a skill").unwrap();
    let skills = tmp.path().join(".claude/skills");
    std::fs::create_dir_all(&skills).unwrap();
    let link = |target: &std::path::Path, name: &str| {
        std::os::unix::fs::symlink(target, skills.join(name)).unwrap();
    };
    link("../../tools/skills/edited".as_ref(), "edited");
    link(outside.path(), "shared");
    commit_days_ago(tmp.path(), 200);
    std::fs::write(tools.join("edited/SKILL.md"), "a skill, revised").unwrap();
    link("../../tools/skills/old".as_ref(), "relinked");
    commit_days_ago(tmp.path(), 1);
    link(outside.path(), "new");

    let changes = last_changes(
        tmp.path(),
        &[
            skills.join("edited"),
            skills.join("relinked"),
            tmp.path().join(".claude/skills/relinked/"),
            skills.join("shared"),
            skills.join("new"),
        ],
    )
    .unwrap();
    let ages: Vec<_> = changes.iter().map(days_since).collect();
    assert!(
        matches!(ages[..3], [Some(0..=1), Some(0..=1), Some(0..=1)])
            && matches!(
                changes[3..],
                [LastChange::Unrecorded, LastChange::Uncommitted]
            ),
        "{changes:?}"
    );

    // A file system that folds case finds the link under any case of its name.
    let folded = skills.join("RELINKED");
    if folded.exists() {
        let changes = last_changes(tmp.path(), &[folded]).unwrap();
        assert!(
            matches!(days_since(&changes[0]), Some(0..=1)),
            "{changes:?}"
        );
    }
}

#[test]
fn sweep_skips_foundation_kinds_and_every_path_they_name() {
    // The foundation glob is carved out of the rule glob, so the same file
    // answers to both kinds; the declaration wins over the broader match,
    // and wins whichever kind is declared first.
    let tmp = repo();
    let rules = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("constitution.md"), "the laws").unwrap();
    std::fs::write(rules.join("naming.md"), "a rule").unwrap();
    let cfg = build_sweep_config(
        tmp.path(),
        vec![
            KindDecl {
                name: "rule".into(),
                glob: ".claude/rules/*.md".into(),
                foundation: false,
                invocation_kind: None,
            },
            KindDecl {
                name: "constitution".into(),
                glob: ".claude/rules/constitution.md".into(),
                foundation: true,
                invocation_kind: None,
            },
        ],
    );
    let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
    let query = TelemetryQuery::new(storage);
    let outcome = RetirementSweeper::new(&cfg, tmp.path(), &query)
        .unwrap()
        .run()
        .unwrap();
    let skip = outcome
        .kinds_skipped
        .iter()
        .find(|s| s.slug == "constitution")
        .expect("foundation kind must be skipped");
    assert!(
        skip.reason.contains("1 path"),
        "the skip says how much the declaration protected: {}",
        skip.reason
    );
    assert!(outcome.kinds_processed.contains(&"rule".to_string()));
    let judged: Vec<&str> = outcome.verdicts.iter().map(|v| v.slug.as_str()).collect();
    assert_eq!(
        judged,
        ["naming"],
        "a file the foundation kind names gets no verdict under any kind"
    );
}

#[test]
fn a_foundation_glob_that_names_nothing_says_so_and_protects_nothing() {
    let tmp = repo();
    let rules = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("constitution.md"), "the laws").unwrap();
    let cfg = build_sweep_config(
        tmp.path(),
        vec![
            KindDecl {
                name: "constitution".into(),
                glob: ".claude/rules/no-such-file.md".into(),
                foundation: true,
                invocation_kind: None,
            },
            KindDecl {
                name: "rule".into(),
                glob: ".claude/rules/*.md".into(),
                foundation: false,
                invocation_kind: None,
            },
        ],
    );
    let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
    let query = TelemetryQuery::new(storage);
    let outcome = RetirementSweeper::new(&cfg, tmp.path(), &query)
        .unwrap()
        .run()
        .unwrap();
    let skip = outcome
        .kinds_skipped
        .iter()
        .find(|s| s.slug == "constitution")
        .expect("the foundation kind is still reported");
    assert!(
        skip.reason.contains("names no path"),
        "a declaration that protected nothing must say so: {}",
        skip.reason
    );
    assert_eq!(
        outcome
            .verdicts
            .iter()
            .map(|v| v.slug.as_str())
            .collect::<Vec<_>>(),
        ["constitution"],
        "and the file it was written for is swept under the kind that matches it"
    );
}

#[test]
fn sweep_skips_kind_without_consumer_detector() {
    let tmp = repo();
    let cfg = build_sweep_config(
        tmp.path(),
        vec![
            KindDecl {
                name: "rule".into(),
                glob: ".claude/rules/*.md".into(),
                foundation: false,
                invocation_kind: None,
            },
            KindDecl {
                name: "skill".into(),
                glob: ".claude/skills/*/SKILL.md".into(),
                foundation: false,
                invocation_kind: None,
            },
        ],
    );
    let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
    let query = TelemetryQuery::new(storage);
    let outcome = RetirementSweeper::new(&cfg, tmp.path(), &query)
        .unwrap()
        .run()
        .unwrap();
    assert!(outcome.kinds_skipped.iter().any(|s| s.slug == "skill"));
    assert!(outcome.kinds_processed.contains(&"rule".to_string()));
}

#[test]
fn sweep_derives_silent_from_telemetry_payload() {
    let tmp = repo();
    let rule_dir = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rule_dir).unwrap();
    std::fs::write(rule_dir.join("active-rule.md"), "x").unwrap();
    std::fs::write(rule_dir.join("silent-rule.md"), "x").unwrap();

    let cfg = build_sweep_config(
        tmp.path(),
        vec![KindDecl {
            name: "rule".into(),
            glob: ".claude/rules/*.md".into(),
            foundation: false,
            invocation_kind: Some("skill-invoked".into()),
        }],
    );

    // Seed telemetry: an event with "active-rule" in payload
    {
        let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
        let mut appender =
            TelemetryAppender::new(cfg.telemetry.as_ref().unwrap(), storage).unwrap();
        appender
            .append(
                "skill-invoked",
                serde_json::json!({"skill": "active-rule", "outcome": "ok"}),
            )
            .unwrap();
    }
    let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
    let query = TelemetryQuery::new(storage);
    let outcome = RetirementSweeper::new(&cfg, tmp.path(), &query)
        .unwrap()
        .run()
        .unwrap();

    let active = outcome
        .verdicts
        .iter()
        .find(|v| v.slug == "active-rule")
        .unwrap();
    let silent_rule = outcome
        .verdicts
        .iter()
        .find(|v| v.slug == "silent-rule")
        .unwrap();
    assert_eq!(
        active.silence,
        SilenceState::Active,
        "active-rule must be Active (an in-window event references it)"
    );
    assert_eq!(
        silent_rule.silence,
        SilenceState::Silent,
        "silent-rule must be Silent (the ledger is live but no event references it)"
    );
}

#[test]
fn sweep_reads_only_the_declared_invocation_kind() {
    // A domain Kind's payload may carry any string, including one that happens
    // to equal a slug. Read as an invocation it would both revive `orphan-rule`
    // and convict `other-rule` of silence — a verdict decided by a coincidence.
    // Only the declared invocation Kind is the record, so both stay Unmeasured.
    let tmp = repo();
    let rule_dir = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rule_dir).unwrap();
    std::fs::write(rule_dir.join("orphan-rule.md"), "body").unwrap();
    std::fs::write(rule_dir.join("other-rule.md"), "body").unwrap();
    let cfg = build_sweep_config(
        tmp.path(),
        vec![KindDecl {
            name: "rule".into(),
            glob: ".claude/rules/*.md".into(),
            foundation: false,
            invocation_kind: Some("skill-invoked".into()),
        }],
    );
    {
        let mut storage = JsonlStorage::new(tmp.path().join("tele"), 10);
        storage
            .append(&Event {
                kind: "deploy".into(),
                timestamp: Timestamp::now(),
                payload: serde_json::json!({"area": "orphan-rule", "status": "ok"}),
            })
            .unwrap();
    }
    let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
    let query = TelemetryQuery::new(storage);
    let outcome = RetirementSweeper::new(&cfg, tmp.path(), &query)
        .unwrap()
        .run()
        .unwrap();
    assert_eq!(outcome.verdicts.len(), 2);
    for v in &outcome.verdicts {
        assert_eq!(v.silence, SilenceState::Unmeasured, "{}", v.slug);
        assert!(!v.signals.contains(&RetirementSignal::Silent));
    }
}

#[test]
fn a_live_record_convicts_only_the_kinds_that_declared_it() {
    // An invocation record names one class of artifact. A rule is loaded, not
    // invoked, so it is never in one — reading its absence there would retire
    // every rule in the project the moment one skill runs. The rule kind
    // declares no record and stays unmeasured while the skill kind measures.
    let tmp = repo();
    std::fs::create_dir_all(tmp.path().join(".claude/rules")).unwrap();
    std::fs::create_dir_all(tmp.path().join(".claude/skills")).unwrap();
    std::fs::write(tmp.path().join(".claude/rules/jiff-time.md"), "body").unwrap();
    std::fs::create_dir_all(tmp.path().join(".claude/skills/review-lenses")).unwrap();
    std::fs::create_dir_all(tmp.path().join(".claude/skills/unused-skill")).unwrap();

    let mut cfg = build_sweep_config(
        tmp.path(),
        vec![
            KindDecl {
                name: "rule".into(),
                glob: ".claude/rules/*.md".into(),
                foundation: false,
                invocation_kind: None,
            },
            KindDecl {
                name: "skill".into(),
                glob: ".claude/skills/*".into(),
                foundation: false,
                invocation_kind: Some("skill-invoked".into()),
            },
        ],
    );
    cfg.lifecycle
        .as_mut()
        .unwrap()
        .consumer_detectors
        .push(ConsumerDetectorDecl {
            kind: "skill".into(),
            pattern: "{slug}".into(),
            exclude_globs: vec![],
        });
    cfg.validate().unwrap();

    {
        let mut appender = TelemetryAppender::new(
            cfg.telemetry.as_ref().unwrap(),
            JsonlStorage::new(tmp.path().join("tele"), 10),
        )
        .unwrap();
        appender
            .append(
                "skill-invoked",
                serde_json::json!({"skill": "review-lenses", "outcome": "ok"}),
            )
            .unwrap();
    }
    let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
    let query = TelemetryQuery::new(storage);
    let outcome = RetirementSweeper::new(&cfg, tmp.path(), &query)
        .unwrap()
        .run()
        .unwrap();

    let find = |slug: &str| {
        outcome
            .verdicts
            .iter()
            .find(|v| v.slug == slug)
            .unwrap_or_else(|| panic!("no verdict for {slug}"))
            .silence
    };
    assert_eq!(find("jiff-time"), SilenceState::Unmeasured);
    assert_eq!(find("review-lenses"), SilenceState::Active);
    assert_eq!(find("unused-skill"), SilenceState::Silent);
}

#[test]
fn sweep_leaves_every_slug_unmeasured_without_a_declared_invocation_kind() {
    // Nothing declares where invocations are recorded, so silence has no
    // oracle to be read from — never a Silent inferred from the ledger at large.
    let tmp = repo();
    let rule_dir = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rule_dir).unwrap();
    std::fs::write(rule_dir.join("orphan-rule.md"), "body").unwrap();
    let cfg = build_sweep_config(
        tmp.path(),
        vec![KindDecl {
            name: "rule".into(),
            glob: ".claude/rules/*.md".into(),
            foundation: false,
            invocation_kind: None,
        }],
    );
    {
        let mut appender = TelemetryAppender::new(
            cfg.telemetry.as_ref().unwrap(),
            JsonlStorage::new(tmp.path().join("tele"), 10),
        )
        .unwrap();
        appender
            .append(
                "skill-invoked",
                serde_json::json!({"skill": "orphan-rule", "outcome": "ok"}),
            )
            .unwrap();
    }
    let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
    let query = TelemetryQuery::new(storage);
    let outcome = RetirementSweeper::new(&cfg, tmp.path(), &query)
        .unwrap()
        .run()
        .unwrap();
    assert_eq!(outcome.verdicts[0].silence, SilenceState::Unmeasured);
}

#[test]
fn config_rejects_a_measured_kind_whose_glob_collapses_every_slug() {
    // `*/SKILL.md` gives every match the stem `SKILL`, so N artifacts share
    // one identity the record names individually — including the one whose
    // invocation is in the ledger. Rejected at load, per Article IV.
    let tmp = TempDir::new().unwrap();
    let mut cfg = build_sweep_config(
        tmp.path(),
        vec![KindDecl {
            name: "rule".into(),
            glob: ".claude/rules/*.md".into(),
            foundation: false,
            invocation_kind: None,
        }],
    );
    cfg.kinds.push(KindDecl {
        name: "skill".into(),
        glob: ".claude/skills/*/SKILL.md".into(),
        foundation: false,
        invocation_kind: Some("skill-invoked".into()),
    });
    let err = cfg.validate().unwrap_err();
    assert_eq!(err.code(), harness_core::error::ErrorCode::ConfigInvalid);

    // No false positive: a varying final component, and a wholly literal glob
    // (which matches at most one file), both stand.
    for glob in [".claude/skills/*", ".claude/rules/constitution.md"] {
        cfg.kinds.last_mut().unwrap().glob = glob.into();
        cfg.validate()
            .unwrap_or_else(|e| panic!("glob '{glob}' must validate: {e}"));
    }
}

#[test]
fn config_rejects_an_unknown_key_on_a_kind() {
    // A misspelled `invocation_kind` would otherwise load clean and drop the
    // kind's whole silence measurement with no error (Article V).
    let toml = common::pinned(
        r#"
        [[kinds]]
        name = "skill"
        glob = ".claude/skills/*"
        invokation_kind = "harness_invocation"
    "#,
    );
    let err = toml::from_str::<Config>(&toml).unwrap_err();
    assert!(
        err.to_string().contains("invokation_kind"),
        "the unknown key must be named: {err}"
    );
}

#[test]
fn config_rejects_an_invocation_kind_no_telemetry_kind_declares() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = build_sweep_config(
        tmp.path(),
        vec![KindDecl {
            name: "rule".into(),
            glob: ".claude/rules/*.md".into(),
            foundation: false,
            invocation_kind: None,
        }],
    );
    cfg.kinds[0].invocation_kind = Some("never-declared".into());
    let err = cfg.validate().unwrap_err();
    assert_eq!(err.code(), harness_core::error::ErrorCode::ConfigInvalid);
}

#[test]
fn sweep_window_override_changes_silent_horizon() {
    let tmp = repo();
    let rule_dir = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rule_dir).unwrap();
    std::fs::write(rule_dir.join("x.md"), "body").unwrap();
    let cfg = build_sweep_config(
        tmp.path(),
        vec![KindDecl {
            name: "rule".into(),
            glob: ".claude/rules/*.md".into(),
            foundation: false,
            invocation_kind: Some("skill-invoked".into()),
        }],
    );
    // One event five days old naming the slug. The window decides whether it
    // is in view: a wide window sees it (Active); a narrow one leaves the
    // ledger empty within the horizon (Unmeasured) — never a fabricated Silent.
    {
        let mut storage = JsonlStorage::new(tmp.path().join("tele"), 10);
        storage
            .append(&Event {
                kind: "skill-invoked".into(),
                timestamp: Timestamp::now() - SignedDuration::from_hours(5 * 24),
                payload: serde_json::json!({"skill": "x", "outcome": "ok"}),
            })
            .unwrap();
    }
    let silence_at = |window: u32| {
        let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
        let query = TelemetryQuery::new(storage);
        RetirementSweeper::new(&cfg, tmp.path(), &query)
            .unwrap()
            .with_silence_window(window)
            .run()
            .unwrap()
            .verdicts[0]
            .silence
    };
    assert_eq!(silence_at(10), SilenceState::Active);
    assert_eq!(silence_at(1), SilenceState::Unmeasured);
}
