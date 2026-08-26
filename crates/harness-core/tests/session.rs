use harness_core::config::SessionConfig;
use harness_core::error::ErrorCode;
use harness_core::session::{self, Authorship, CollectOptions};
use tempfile::TempDir;

/// A transcript line the runtime attributes to a person typing.
fn typed(session: &str, uuid: &str, ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","version":"2.1.246","origin":{{"kind":"human"}},"promptSource":"typed","message":{{"content":{}}}}}"#,
        serde_json::to_string(text).unwrap()
    )
}

/// A transcript line the runtime attributes to nobody — the shape an interrupt,
/// a resumption or an injected caveat takes.
fn unclaimed(session: &str, uuid: &str, ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","version":"2.1.246","message":{{"content":{}}}}}"#,
        serde_json::to_string(text).unwrap()
    )
}

fn corpus(files: &[(&str, Vec<String>)]) -> (TempDir, SessionConfig) {
    let dir = TempDir::new().unwrap();
    for (name, lines) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, lines.join("\n")).unwrap();
    }
    let config = SessionConfig {
        roots: vec![dir.path().to_string_lossy().into_owned()],
        min_block_chars: 30,
        coverage_floor: 0.95,
        min_support: 1,
        baseline_path: dir.path().join("baselines.jsonl"),
        submission_sample: None,
    };
    (dir, config)
}

const STANDING: &str = "always resolve the root cause rather than reaching for a temporary patch";
const ALSO_STANDING: &str = "never leave a comment explaining what the change used to be";

#[test]
fn the_paragraph_typed_into_the_most_sessions_ranks_first_with_every_citation() {
    let (_dir, config) = corpus(&[
        (
            "-Users-me-alpha/s1.jsonl",
            vec![
                typed(
                    "s1",
                    "a1",
                    "2026-08-01T09:00:00Z",
                    &format!("{STANDING}\n\n{ALSO_STANDING}\n\nrefactor the loader"),
                ),
                typed("s1", "a2", "2026-08-01T10:00:00Z", STANDING),
            ],
        ),
        (
            "-Users-me-beta/s2.jsonl",
            vec![typed(
                "s2",
                "b1",
                "2026-08-02T09:00:00Z",
                &format!("{STANDING}\n\nnow the exporter"),
            )],
        ),
        (
            "-Users-me-beta/s3.jsonl",
            vec![typed(
                "s3",
                "c1",
                "2026-08-03T09:00:00Z",
                &format!("{STANDING}\n\n{ALSO_STANDING}"),
            )],
        ),
    ]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.prompts.authored_turns, 4);
    let blocks = &facts.prompts.repeated_blocks;
    assert_eq!(blocks.len(), 2);

    let first = &blocks[0];
    assert_eq!(first.sessions, 3);
    assert_eq!(first.occurrences, 4);
    assert_eq!(first.citations.len(), 4);
    assert_eq!(first.citations[0].uuid, "a1", "citations run oldest first");
    assert!(first.citations.iter().all(|c| c.file.is_absolute()));

    assert_eq!(blocks[1].sessions, 2);
}

#[test]
fn turns_the_runtime_attributed_to_nobody_leave_the_statistics_alone() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            unclaimed(
                "s1",
                "a2",
                "2026-08-01T09:01:00Z",
                "[Request interrupted by user]",
            ),
            unclaimed("s1", "a3", "2026-08-01T09:02:00Z", STANDING),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.prompts.authored_turns, 1);
    assert_eq!(
        facts
            .coverage
            .user_turns_by_authorship
            .get(Authorship::Unclaimed.as_str()),
        Some(&2)
    );
    assert_eq!(
        facts.coverage.authorship_ratio(),
        Some(1.0),
        "unclaimed turns are outside the ratio, not failures within it"
    );
}

#[cfg(unix)]
#[test]
fn a_run_that_reads_nothing_is_an_error_rather_than_a_report_of_zero() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("s.jsonl");
    std::fs::write(&path, typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

    let config = SessionConfig {
        roots: vec![dir.path().to_string_lossy().into_owned()],
        min_block_chars: 30,
        coverage_floor: 0.95,
        min_support: 1,
        baseline_path: dir.path().join("baselines.jsonl"),
        submission_sample: None,
    };

    let err = session::collect(&config, &CollectOptions::default()).unwrap_err();

    assert_eq!(err.code(), ErrorCode::SessionRootUnreadable);
}

#[test]
fn text_stays_on_disk_unless_the_caller_asks_for_it() {
    let (_dir, config) = corpus(&[
        (
            "-Users-me-alpha/s1.jsonl",
            vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
        ),
        (
            "-Users-me-alpha/s2.jsonl",
            vec![typed("s2", "b1", "2026-08-02T09:00:00Z", STANDING)],
        ),
    ]);

    let withheld = session::collect(&config, &CollectOptions::default()).unwrap();
    assert!(withheld.prompts.repeated_blocks[0].text.is_none());
    let serialised = serde_json::to_string(&withheld).unwrap();
    assert!(
        !serialised.contains(STANDING),
        "the default envelope must not carry what was typed"
    );

    let asked = session::collect(
        &config,
        &CollectOptions {
            with_text: true,
            ..CollectOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        asked.prompts.repeated_blocks[0].text.as_deref(),
        Some(STANDING)
    );
}

#[test]
fn a_record_type_this_binary_does_not_know_is_counted_and_the_run_continues() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            r#"{"type":"a-type-shipped-tomorrow","uuid":"x","timestamp":"2026-08-01T08:00:00Z","sessionId":"s1"}"#.to_string(),
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.prompts.authored_turns, 1);
    assert_eq!(
        facts
            .coverage
            .record_types_unconsumed
            .get("a-type-shipped-tomorrow"),
        Some(&1)
    );
    assert_eq!(facts.coverage.records_malformed, 0);
    assert!(session::require_coverage(&facts.coverage, config.coverage_floor).is_ok());
}

/// A tool-result record reporting an edit, as `Edit` and `Write` write one.
fn edit(session: &str, uuid: &str, ts: &str, path: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","version":"2.1.246","message":{{"content":[{{"type":"tool_result","content":"ok"}}]}},"toolUseResult":{{"filePath":"{path}","structuredPatch":[]}}}}"#
    )
}

/// A tool-result record reporting a commit, as a `Bash` git commit writes one.
fn commit(session: &str, uuid: &str, ts: &str, sha: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","version":"2.1.246","message":{{"content":[{{"type":"tool_result","content":"ok"}}]}},"toolUseResult":{{"gitOperation":{{"commit":{{"sha":"{sha}","kind":"commit","branch":"main"}}}}}}}}"#
    )
}

/// A read of a file reports its path without a patch.
fn read_only(session: &str, uuid: &str, ts: &str, path: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","version":"2.1.246","message":{{"content":[{{"type":"tool_result","content":"ok"}}]}},"toolUseResult":{{"filePath":"{path}"}}}}"#
    )
}

#[test]
fn a_file_touched_again_after_its_commit_surfaces_with_the_commit_that_shipped_it() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            edit("s1", "e1", "2026-08-01T09:05:00Z", "/repo/loader.rs"),
            commit("s1", "c1", "2026-08-01T09:10:00Z", "1111111"),
            edit("s1", "e2", "2026-08-01T09:20:00Z", "/repo/loader.rs"),
            edit("s1", "e3", "2026-08-01T09:25:00Z", "/repo/loader.rs"),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.rework.commits, 1);
    assert_eq!(facts.rework.post_commit_reedits.len(), 1);
    let r = &facts.rework.post_commit_reedits[0];
    assert_eq!(r.commit, "1111111");
    assert_eq!(r.reedits, 2);
    assert_eq!(r.citations[0].uuid, "e2");
}

#[test]
fn reading_a_committed_file_is_not_touching_it_again() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            edit("s1", "e1", "2026-08-01T09:05:00Z", "/repo/loader.rs"),
            commit("s1", "c1", "2026-08-01T09:10:00Z", "1111111"),
            read_only("s1", "r1", "2026-08-01T09:20:00Z", "/repo/loader.rs"),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert!(facts.rework.post_commit_reedits.is_empty());
    assert_eq!(facts.rework.reedits_after_a_later_commit, 0);
}

#[test]
fn queued_turns_fold_into_one_instruction_end_to_end() {
    let queued = |uuid: &str, ts: &str, text: &str| {
        format!(
            r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"s1","version":"2.1.246","origin":{{"kind":"human"}},"promptSource":"queued","message":{{"content":{}}}}}"#,
            serde_json::to_string(text).unwrap()
        )
    };
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            queued("a2", "2026-08-01T09:00:03Z", STANDING),
            typed("s1", "a3", "2026-08-01T11:00:00Z", STANDING),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.prompts.authored_turns, 3);
    assert_eq!(facts.prompts.submissions, 2);
    assert_eq!(
        facts.prompts.restated_blocks[0].submissions, 2,
        "the queued copy rides along; the later one is the restatement"
    );
}

/// The assistant record that makes a tool call, and the denial that answers it.
fn call_and_denial(session: &str, tool: &str, kind: &str, seconds: u32) -> Vec<String> {
    let id = format!("t{seconds}");
    vec![
        format!(
            r#"{{"type":"assistant","uuid":"a{seconds}","timestamp":"2026-08-01T10:00:{seconds:02}Z","sessionId":"{session}","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"{tool}","input":{{"command":"x"}}}}]}}}}"#
        ),
        format!(
            r#"{{"type":"user","uuid":"d{seconds}","timestamp":"2026-08-01T10:00:{seconds:02}Z","sessionId":"{session}","toolDenialKind":"{kind}","message":{{"content":[{{"type":"tool_result","tool_use_id":"{id}","content":"denied"}}]}}}}"#
        ),
    ]
}

fn stop_summary(session: &str, uuid: &str, ts: &str, command: &str, ms: u64) -> String {
    format!(
        r#"{{"type":"system","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","subtype":"stop_hook_summary","hookInfos":[{{"command":"{command}","durationMs":{ms}}}],"hookErrors":[],"preventedContinuation":false}}"#
    )
}

fn rule_load(session: &str, uuid: &str, ts: &str, path: &str, body: &str) -> String {
    format!(
        r#"{{"type":"attachment","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","attachment":{{"type":"nested_memory","path":"{path}","content":{{"content":{}}}}}}}"#,
        serde_json::to_string(body).unwrap()
    )
}

#[test]
fn the_harness_side_separates_who_refused_and_what_each_hook_cost() {
    let mut lines = vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)];
    lines.extend(call_and_denial("s1", "Bash", "permission-rule", 1));
    lines.extend(call_and_denial("s1", "Bash", "permission-rule", 2));
    lines.extend(call_and_denial("s1", "Bash", "user-rejected", 3));
    lines.push(stop_summary(
        "s1",
        "h1",
        "2026-08-01T11:00:00Z",
        "afplay chime &",
        2543,
    ));
    lines.push(stop_summary(
        "s1",
        "h2",
        "2026-08-01T11:10:00Z",
        "afplay chime &",
        2400,
    ));
    lines.push(rule_load(
        "s1",
        "m1",
        "2026-08-01T09:00:01Z",
        "/repo/.claude/rules/testing.md",
        "abcdefghij",
    ));

    let (_dir, config) = corpus(&[("-Users-me-alpha/s1.jsonl", lines)]);
    let facts = session::collect(&config, &CollectOptions::default()).unwrap();
    let h = &facts.harness;

    assert_eq!(h.denials.len(), 2);
    assert_eq!(h.denials[0].kind, "permission-rule");
    assert_eq!(h.denials[0].tool.as_deref(), Some("Bash"));
    assert_eq!(h.denials[0].denials, 2);
    assert_eq!(h.denials[1].kind, "user-rejected");

    assert_eq!(h.stops, 2);
    assert_eq!(h.prevented_continuations, 0);
    assert_eq!(h.hooks[0].command, "afplay chime &");
    assert_eq!(h.hooks[0].total_ms, 4943);

    assert_eq!(h.rule_loads[0].loads, 1);
    assert_eq!(h.rule_loads[0].chars, 10);
}

#[test]
fn an_attachment_this_binary_does_not_consume_stays_visible_in_coverage() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            r#"{"type":"attachment","uuid":"x1","timestamp":"2026-08-01T09:00:01Z","sessionId":"s1","attachment":{"type":"hook_success"}}"#.to_string(),
            r#"{"type":"system","uuid":"x2","timestamp":"2026-08-01T09:00:02Z","sessionId":"s1","subtype":"turn_duration"}"#.to_string(),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(
        facts
            .coverage
            .record_types_unconsumed
            .get("attachment:hook_success"),
        Some(&1)
    );
    assert_eq!(
        facts
            .coverage
            .record_types_unconsumed
            .get("system:turn_duration"),
        Some(&1)
    );
    assert_eq!(facts.coverage.records_malformed, 0);
}

fn baseline_of(config: &SessionConfig, since: Option<&str>, label: &str) -> session::Baseline {
    let facts = session::collect(
        config,
        &CollectOptions {
            with_text: false,
            since: since.map(|s| s.parse().unwrap()),
            ..CollectOptions::default()
        },
    )
    .unwrap();
    session::Baseline::of(label, "2026-09-01T00:00:00Z".parse().unwrap(), None, &facts)
}

#[test]
fn a_window_measured_after_the_last_one_ended_compares_against_it() {
    let (dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            typed("s1", "a2", "2026-08-01T10:00:00Z", STANDING),
        ],
    )]);

    let before = baseline_of(&config, None, "before");
    let ledger = session::BaselineLedger::new(config.baseline_path.clone());
    ledger.append(&before).unwrap();

    std::fs::write(
        dir.path().join("-Users-me-alpha/s2.jsonl"),
        [
            typed("s2", "b1", "2026-08-05T09:00:00Z", STANDING),
            typed("s2", "b2", "2026-08-05T10:00:00Z", ALSO_STANDING),
        ]
        .join("\n"),
    )
    .unwrap();

    let resume = session::baseline::latest_observed_to(&ledger.load_all().unwrap(), None).unwrap();
    let after = baseline_of(&config, Some(&resume.to_string()), "after");
    ledger.append(&after).unwrap();

    let recorded = ledger.load_all().unwrap();
    let (from, to) = session::baseline::select(&recorded, None, None).unwrap();
    assert_eq!(
        (from.label.as_str(), to.label.as_str()),
        ("before", "after")
    );

    let diff = session::baseline::diff(from, to, config.min_support).unwrap();
    let restated = diff
        .metrics
        .iter()
        .find(|m| m.metric == "restated_chars_per_submission")
        .expect("metric present on both sides");
    assert_eq!(restated.from.numerator, STANDING.len() as u64);
    assert_eq!(restated.to.numerator, 0);
    assert!(restated.change.is_some_and(|c| c < 0.0));
}

#[test]
fn two_windows_over_the_same_history_are_refused_rather_than_diluted() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            typed("s1", "a2", "2026-08-05T10:00:00Z", STANDING),
        ],
    )]);

    let before = baseline_of(&config, None, "before");
    let after = baseline_of(&config, None, "after");

    let err = session::baseline::diff(&before, &after, config.min_support).unwrap_err();

    assert_eq!(err.code(), ErrorCode::SessionBaselineNotComparable);
}

#[test]
fn a_label_the_ledger_already_holds_is_refused() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
    )]);
    let ledger = session::BaselineLedger::new(config.baseline_path.clone());
    ledger
        .append(&baseline_of(&config, None, "before"))
        .unwrap();

    let err = ledger
        .append(&baseline_of(&config, None, "before"))
        .unwrap_err();

    assert_eq!(err.code(), ErrorCode::SessionBaselineLabelRejected);
    assert_eq!(ledger.load_all().unwrap().len(), 1);
}

#[test]
fn a_rate_under_the_support_floor_keeps_both_sides_and_withholds_the_subtraction() {
    let (dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
    )]);
    let before = baseline_of(&config, None, "before");
    std::fs::write(
        dir.path().join("-Users-me-alpha/s2.jsonl"),
        typed("s2", "b1", "2026-08-05T09:00:00Z", STANDING),
    )
    .unwrap();
    let after = baseline_of(&config, Some("2026-08-05T00:00:00Z"), "after");

    let diff = session::baseline::diff(&before, &after, 30).unwrap();

    let restated = diff
        .metrics
        .iter()
        .find(|m| m.metric == "restated_chars_per_submission")
        .unwrap();
    assert_eq!(restated.from.denominator, 1);
    assert_eq!(restated.to.denominator, 1);
    assert!(restated.change.is_none());
}

#[test]
fn a_metric_only_one_side_carries_is_named_rather_than_filled_in() {
    let (dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
    )]);
    let mut before = baseline_of(&config, None, "before");
    before.measurements.remove("denials_per_submission");
    std::fs::write(
        dir.path().join("-Users-me-alpha/s2.jsonl"),
        typed("s2", "b1", "2026-08-05T09:00:00Z", STANDING),
    )
    .unwrap();
    let after = baseline_of(&config, Some("2026-08-05T00:00:00Z"), "after");

    let diff = session::baseline::diff(&before, &after, config.min_support).unwrap();

    assert_eq!(diff.metrics_unmatched, vec!["denials_per_submission"]);
    assert!(
        diff.metrics
            .iter()
            .all(|m| m.metric != "denials_per_submission")
    );
}

#[test]
fn a_corrupt_ledger_line_stops_the_read_rather_than_shortening_the_history() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
    )]);
    let ledger = session::BaselineLedger::new(config.baseline_path.clone());
    ledger
        .append(&baseline_of(&config, None, "before"))
        .unwrap();
    let mut body = std::fs::read_to_string(&config.baseline_path).unwrap();
    body.push_str("{\"label\":\"truncated\"}\n");
    std::fs::write(&config.baseline_path, body).unwrap();

    let err = ledger.load_all().unwrap_err();

    assert_eq!(err.code(), ErrorCode::IoFailure);
}

#[test]
fn coverage_counts_the_window_rather_than_the_whole_file() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            typed("s1", "a2", "2026-08-05T09:00:00Z", ALSO_STANDING),
        ],
    )]);
    let options = CollectOptions {
        with_text: false,
        since: Some("2026-08-05T00:00:00Z".parse().unwrap()),
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(facts.coverage.records_total, 1);
    assert_eq!(
        facts.coverage.observed_from,
        Some("2026-08-05T09:00:00Z".parse().unwrap())
    );
    assert_eq!(facts.coverage.observed_to, facts.coverage.observed_from);
}

/// A turn the operator sent while the agent was working.
fn queued(session: &str, uuid: &str, ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","version":"2.1.246","origin":{{"kind":"human"}},"promptSource":"queued","message":{{"content":{}}}}}"#,
        serde_json::to_string(text).unwrap()
    )
}

/// An agent turn that produced text and nothing else.
fn spoke(session: &str, uuid: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","message":{{"content":[{{"type":"text","text":"working on it"}}]}}}}"#
    )
}

/// The record the runtime writes where it cut an agent turn short.
fn marked_interrupt(session: &str, uuid: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","interruptedMessageId":"msg_01","message":{{"content":"[Request interrupted by user]"}}}}"#
    )
}

#[test]
fn a_queued_turn_after_the_agent_has_spoken_is_a_new_instruction_and_steering() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            spoke("s1", "x1", "2026-08-01T09:00:05Z"),
            queued("s1", "a2", "2026-08-01T09:00:09Z", ALSO_STANDING),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.prompts.submissions, 2);
    assert_eq!(facts.interventions.by_kind["steering"], 1);
    assert_eq!(facts.interventions.interventions[0].citation.uuid, "a2");
}

#[test]
fn a_tool_result_is_not_the_operator_speaking() {
    let mut lines = vec![
        typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
        spoke("s1", "x1", "2026-08-01T09:00:05Z"),
    ];
    lines.extend(call_and_denial("s1", "Bash", "permission-rule", 6));
    lines.push(queued("s1", "a2", "2026-08-01T10:00:09Z", ALSO_STANDING));
    let (_dir, config) = corpus(&[("-Users-me-alpha/s1.jsonl", lines)]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(
        facts.interventions.by_kind["steering"], 1,
        "the denial record sits between them but the operator did not type it"
    );
}

#[test]
fn a_refused_tool_call_is_not_counted_as_an_intervention() {
    let mut lines = vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)];
    lines.extend(call_and_denial("s1", "Bash", "user-rejected", 7));
    let (_dir, config) = corpus(&[("-Users-me-alpha/s1.jsonl", lines)]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert!(facts.interventions.interventions.is_empty());
    assert_eq!(facts.harness.denials[0].kind, "user-rejected");
}

#[test]
fn an_interrupt_the_runtime_marked_is_reported_though_no_person_authored_it() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            spoke("s1", "x1", "2026-08-01T09:00:05Z"),
            marked_interrupt("s1", "i1", "2026-08-01T09:00:07Z"),
            typed("s1", "a2", "2026-08-01T09:00:20Z", ALSO_STANDING),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.interventions.by_kind["marked-interrupt"], 1);
    assert_eq!(facts.interventions.by_kind["steering"], 0);
    assert_eq!(
        facts.coverage.user_turns_by_authorship[Authorship::Unclaimed.as_str()],
        1,
        "the marker stays outside the coverage denominator"
    );
}

#[test]
fn an_instruction_carries_what_happened_while_it_stood() {
    let mut lines = vec![
        typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
        spoke("s1", "x1", "2026-08-01T09:00:05Z"),
    ];
    lines.extend(call_and_denial("s1", "Bash", "permission-rule", 6));
    lines.push(marked_interrupt("s1", "i1", "2026-08-01T10:00:08Z"));
    let (_dir, config) = corpus(&[("-Users-me-alpha/s1.jsonl", lines)]);
    let options = CollectOptions {
        with_submissions: true,
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    let only = &facts.submissions[0];
    assert_eq!(only.citation.uuid, "a1");
    assert_eq!(only.turns, 1);
    assert_eq!(only.agent_turns, 2, "the reply and the tool call");
    assert_eq!(only.denials, 1);
    assert_eq!(only.interrupts, 1);
    assert!(!only.steered_away);
    assert!(only.text.is_none(), "text stays on disk unless asked for");
}

#[test]
fn an_instruction_the_operator_did_not_wait_for_is_marked_where_it_was_cut() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            spoke("s1", "x1", "2026-08-01T09:00:05Z"),
            queued("s1", "a2", "2026-08-01T09:00:09Z", ALSO_STANDING),
            spoke("s1", "x2", "2026-08-01T09:00:20Z"),
        ],
    )]);
    let options = CollectOptions {
        with_submissions: true,
        with_text: true,
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(facts.submissions.len(), 2);
    assert!(facts.submissions[0].steered_away);
    assert!(!facts.submissions[1].steered_away);
    assert_eq!(facts.submissions[1].text.as_deref(), Some(ALSO_STANDING));
}

#[test]
fn the_instruction_list_is_absent_unless_the_caller_asks() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert!(facts.submissions.is_empty());
    assert_eq!(facts.prompts.submissions, 1, "the count is always there");
}

/// An agent turn that stopped to ask rather than choose.
fn asked(session: &str, uuid: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","message":{{"content":[{{"type":"tool_use","id":"q{uuid}","name":"AskUserQuestion","input":{{"questions":[]}}}}]}}}}"#
    )
}

#[test]
fn an_instruction_carries_the_work_done_under_it_not_the_session_total() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            asked("s1", "x1", "2026-08-01T09:00:02Z"),
            edit("s1", "e1", "2026-08-01T09:00:05Z", "/p/src/loader.rs"),
            edit("s1", "e2", "2026-08-01T09:00:06Z", "/p/src/loader.rs"),
            edit("s1", "e3", "2026-08-01T09:00:07Z", "/p/src/exporter.rs"),
            commit("s1", "c1", "2026-08-01T09:00:09Z", "abc1234"),
            typed("s1", "a2", "2026-08-01T10:00:00Z", ALSO_STANDING),
            edit("s1", "e4", "2026-08-01T10:00:03Z", "/p/src/loader.rs"),
        ],
    )]);
    let options = CollectOptions {
        with_submissions: true,
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    let (first, second) = (&facts.submissions[0], &facts.submissions[1]);
    assert_eq!((first.edits, first.files, first.commits), (3, 2, 1));
    assert_eq!(first.questions, 1);
    assert_eq!(
        (second.edits, second.files, second.commits),
        (1, 1, 0),
        "the second instruction carries its own work, not the first's"
    );
}

/// A turn carrying the directory the session ran in.
fn typed_in(session: &str, uuid: &str, ts: &str, cwd: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","version":"2.1.246","cwd":"{cwd}","origin":{{"kind":"human"}},"promptSource":"typed","message":{{"content":{}}}}}"#,
        serde_json::to_string(text).unwrap()
    )
}

#[test]
fn a_project_window_admits_a_worktree_below_it_and_nothing_beside_it() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed_in("s1", "a1", "2026-08-01T09:00:00Z", "/w/alpha", STANDING),
            typed_in(
                "s2",
                "a2",
                "2026-08-01T09:10:00Z",
                "/w/alpha/.claude/worktrees/fix",
                STANDING,
            ),
            typed_in(
                "s3",
                "a3",
                "2026-08-01T09:20:00Z",
                "/w/alpha-tools",
                STANDING,
            ),
            typed_in("s4", "a4", "2026-08-01T09:30:00Z", "/w/beta", STANDING),
        ],
    )]);
    let options = CollectOptions {
        project: Some("/w/alpha".into()),
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(
        facts.prompts.submissions, 2,
        "the project and its worktree, not the sibling whose name starts the same"
    );
    assert_eq!(facts.coverage.records_total, 2);
}

#[test]
fn baselines_measured_over_different_scopes_are_not_subtracted() {
    let (dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![typed_in(
            "s1",
            "a1",
            "2026-08-01T09:00:00Z",
            "/w/alpha",
            STANDING,
        )],
    )]);
    let whole = baseline_of(&config, None, "whole");
    std::fs::write(
        dir.path().join("-Users-me-alpha/s2.jsonl"),
        typed_in("s2", "b1", "2026-08-05T09:00:00Z", "/w/alpha", STANDING),
    )
    .unwrap();
    let facts = session::collect(
        &config,
        &CollectOptions {
            since: Some("2026-08-05T00:00:00Z".parse().unwrap()),
            project: Some("/w/alpha".into()),
            ..CollectOptions::default()
        },
    )
    .unwrap();
    let scoped = session::Baseline::of(
        "scoped",
        "2026-09-01T00:00:00Z".parse().unwrap(),
        Some("/w/alpha".into()),
        &facts,
    );

    let err = session::baseline::diff(&whole, &scoped, config.min_support).unwrap_err();

    assert_eq!(err.code(), ErrorCode::SessionBaselineNotComparable);
}
