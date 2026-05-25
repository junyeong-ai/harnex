//! Integration tests for the lifecycle module.

use harness_core::config::{Config, TelemetryConfig, TelemetryKindDecl};
use harness_core::config::{
    ConsumerDetectorDecl, KindDecl, LifecycleConfig, RetirementConfig, RetirementExemptDecl,
};
use harness_core::lifecycle::{
    ConsumerDetector, DecisionLedger, GrepConsumerDetector, LifecycleDecisionRecorder,
    ObservationLedger, PromotionCandidateFinder, PromotionDecision, RetirementClassifier,
    RetirementSignal, RetirementSweeper, consumer_detector_for,
};
use harness_core::telemetry::{JsonlStorage, TelemetryAppender, TelemetryQuery};
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

/// Test-only convenience: bundles the read-only Finder and the write
/// Recorder into one struct, since most lifecycle tests need both.
struct TestPromoter<'a> {
    finder: PromotionCandidateFinder<'a>,
    recorder: LifecycleDecisionRecorder<'a>,
}

fn mk_promoter<'a>(
    cfg: &'a LifecycleConfig,
    observations: &'a ObservationLedger,
    decisions: &'a DecisionLedger,
) -> TestPromoter<'a> {
    TestPromoter {
        finder: PromotionCandidateFinder::new(cfg, observations, decisions),
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
    let candidates = promoter.finder.list_candidates().unwrap();
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
        .finder
        .list_candidates()
        .unwrap();
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
        .finder
        .list_candidates()
        .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].instance_count, 3);
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
        .classify("rule", &rule_path, detector.as_ref(), true)
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
        .classify("rule", &rule_path, detector.as_ref(), true)
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
    assert_eq!(promoter.finder.list_candidates().unwrap().len(), 1);
    promoter
        .recorder
        .promote(
            "naming",
            "use snake case",
            "promoted to naming-conventions.md",
        )
        .unwrap();
    assert!(promoter.finder.list_candidates().unwrap().is_empty());
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
    assert!(promoter.finder.list_candidates().unwrap().is_empty());
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
    assert_eq!(promoter.finder.list_candidates().unwrap().len(), 1);
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
    assert!(promoter.finder.list_candidates().unwrap().is_empty());
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
    assert!(promoter.finder.list_candidates().unwrap().is_empty());
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
            harnex_version: ">=0.1, <0.2".into(),
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
        }],
    );
    let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
    let query = TelemetryQuery::new(storage);

    let sweep = RetirementSweeper::new(&cfg, tmp.path(), &query).unwrap();
    let outcome = sweep.run().unwrap();
    assert_eq!(outcome.files_classified, 2);
    assert!(outcome.kinds_processed.contains(&"rule".to_string()));
    // Both files: no consumer, no recent edit telemetry → Silent + NoConsumers signals
    for v in &outcome.verdicts {
        assert!(v.signals.contains(&RetirementSignal::NoConsumers));
        assert!(v.signals.contains(&RetirementSignal::Silent));
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
            },
            KindDecl {
                name: "rule".into(),
                glob: ".claude/rules/*.md".into(),
                foundation: false,
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
            },
            KindDecl {
                name: "skill".into(),
                glob: ".claude/skills/*/SKILL.md".into(),
                foundation: false,
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
    assert!(
        !active.silent,
        "active-rule must not be Silent (event references it)"
    );
    assert!(
        silent_rule.silent,
        "silent-rule must be Silent (no event references)"
    );
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
        }],
    );
    // No telemetry seeded — every slug Silent regardless of window
    let storage = JsonlStorage::new(tmp.path().join("tele"), 10);
    let query = TelemetryQuery::new(storage);
    let outcome = RetirementSweeper::new(&cfg, tmp.path(), &query)
        .unwrap()
        .with_silence_window(1)
        .run()
        .unwrap();
    assert!(outcome.verdicts.iter().all(|v| v.silent));
}
