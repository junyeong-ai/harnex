//! Integration tests for the lifecycle module.

use harness_core::config::{Config, TelemetryConfig, TelemetryKindDecl};
use harness_core::config::{
    ConsumerDetectorDecl, KindDecl, LifecycleConfig, RetirementConfig, RetirementExemptDecl,
};
use harness_core::error::Error;
use harness_core::lifecycle::{
    ConsumerDetector, DecisionLedger, GrepConsumerDetector, LedgerReader,
    LifecycleDecisionRecorder, ObservationLedger, PromotionDecision, RetirementClassifier,
    RetirementSignal, RetirementSweeper, SilenceState, consumer_detector_for,
};
use harness_core::telemetry::{Event, JsonlStorage, TelemetryAppender, TelemetryQuery};
use jiff::{SignedDuration, Timestamp};
use std::path::PathBuf;
use tempfile::TempDir;

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
            strategy: "grep".into(),
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
    let tmp = TempDir::new().unwrap();
    let rule_dir = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rule_dir).unwrap();
    std::fs::write(rule_dir.join("my-rule.md"), "rule body").unwrap();
    let consumer1 = tmp.path().join("docs/spec.md");
    std::fs::create_dir_all(consumer1.parent().unwrap()).unwrap();
    std::fs::write(&consumer1, "see my-rule for details").unwrap();
    let unrelated = tmp.path().join("docs/other.md");
    std::fs::write(&unrelated, "irrelevant content").unwrap();

    let detector = GrepConsumerDetector::new(
        ConsumerDetectorDecl {
            kind: "rule".into(),
            strategy: "grep".into(),
            pattern: "{slug}".into(),
            exclude_globs: vec![".claude/rules/{slug}.md".into()],
        },
        tmp.path().to_path_buf(),
    );
    let consumers = detector.find_consumers("my-rule").unwrap();
    assert_eq!(consumers.len(), 1);
    assert!(consumers[0].to_string_lossy().ends_with("spec.md"));
}

#[test]
fn consumer_detector_prunes_skip_dirs() {
    // A match inside a build/binary skip dir (e.g. `target/`) must not count
    // as a consumer — and the prune happens before descent, so an unreadable
    // skip dir would never abort the sweep either.
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("target/debug");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("build.log"), "references my-rule here").unwrap();
    let real = tmp.path().join("docs/spec.md");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, "see my-rule").unwrap();

    let detector = GrepConsumerDetector::new(
        ConsumerDetectorDecl {
            kind: "rule".into(),
            strategy: "grep".into(),
            pattern: "{slug}".into(),
            exclude_globs: vec![],
        },
        tmp.path().to_path_buf(),
    );
    let consumers = detector.find_consumers("my-rule").unwrap();
    assert_eq!(
        consumers.len(),
        1,
        "target/ match must be pruned: {consumers:?}"
    );
    assert!(consumers[0].to_string_lossy().ends_with("spec.md"));
}

#[test]
fn retirement_classifier_marks_no_consumers_silent() {
    let tmp = TempDir::new().unwrap();
    let rule_dir = tmp.path().join(".claude/rules");
    std::fs::create_dir_all(&rule_dir).unwrap();
    let rule_path = rule_dir.join("orphan-rule.md");
    std::fs::write(&rule_path, "body").unwrap();
    let cfg = default_lifecycle(tmp.path().join(".harness/observations"));
    let detector = consumer_detector_for(cfg.consumer_detectors[0].clone(), tmp.path()).unwrap();
    let classifier = RetirementClassifier::new(&cfg, None);
    let verdict = classifier
        .classify("rule", &rule_path, detector.as_ref(), SilenceState::Silent)
        .unwrap();
    assert!(verdict.signals.contains(&RetirementSignal::NoConsumers));
    assert!(verdict.signals.contains(&RetirementSignal::Silent));
}

#[test]
fn retirement_classifier_honors_exempt_list() {
    let tmp = TempDir::new().unwrap();
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
    let detector = consumer_detector_for(cfg.consumer_detectors[0].clone(), tmp.path()).unwrap();
    let classifier = RetirementClassifier::new(&cfg, Some(&retire_cfg));
    let verdict = classifier
        .classify("rule", &rule_path, detector.as_ref(), SilenceState::Silent)
        .unwrap();
    assert!(verdict.exempt);
}

#[test]
fn consumer_factory_rejects_unknown_strategy() {
    let tmp = TempDir::new().unwrap();
    let result = consumer_detector_for(
        ConsumerDetectorDecl {
            kind: "rule".into(),
            strategy: "made-up".into(),
            pattern: "{slug}".into(),
            exclude_globs: vec![],
        },
        tmp.path(),
    );
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("expected unknown-strategy error"),
    };
    assert_eq!(
        err.code(),
        harness_core::error::ErrorCode::LifecycleConsumerStrategyUnknown
    );
}

#[test]
fn consumer_factory_builds_graph_backlinks_when_nodex_present() {
    let tmp = TempDir::new().unwrap();
    let result = consumer_detector_for(
        ConsumerDetectorDecl {
            kind: "rule".into(),
            strategy: "graph-backlinks".into(),
            pattern: "rule-{slug}".into(),
            exclude_globs: vec![],
        },
        tmp.path(),
    );
    match result {
        Ok(detector) => assert_eq!(detector.strategy(), "graph-backlinks"),
        Err(e) => assert_eq!(e.code(), harness_core::error::ErrorCode::GraphSpawnFailure),
    }
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
            harnex_version: ">=0.5, <0.6".into(),
        },
        kinds: extra_kinds,
        evidence: None,
        telemetry: Some(telemetry_cfg(tmp_path.join("tele"))),
        codegen: None,
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
    let tmp = TempDir::new().unwrap();
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

#[test]
fn sweep_skips_foundation_kinds() {
    let tmp = TempDir::new().unwrap();
    let cfg = build_sweep_config(
        tmp.path(),
        vec![
            KindDecl {
                name: "constitution".into(),
                glob: ".claude/rules/constitution.md".into(),
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
    assert!(
        outcome
            .kinds_skipped
            .iter()
            .any(|s| s.slug == "constitution"),
        "foundation kind must be skipped"
    );
    assert!(outcome.kinds_processed.contains(&"rule".to_string()));
}

#[test]
fn sweep_skips_kind_without_consumer_detector() {
    let tmp = TempDir::new().unwrap();
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
    let tmp = TempDir::new().unwrap();
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
    let tmp = TempDir::new().unwrap();
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
    let tmp = TempDir::new().unwrap();
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
            strategy: "grep".into(),
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
    let tmp = TempDir::new().unwrap();
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
    let toml = r#"
        [meta]
        harnex_version = ">=0.5, <0.6"
        [[kinds]]
        name = "skill"
        glob = ".claude/skills/*"
        invokation_kind = "harness_invocation"
    "#;
    let err = toml::from_str::<Config>(toml).unwrap_err();
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
    let tmp = TempDir::new().unwrap();
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
