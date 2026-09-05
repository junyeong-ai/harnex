use std::path::PathBuf;

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

/// The copy the runtime writes into a forked session's transcript: the earlier
/// session's record verbatim, restamped with the new session's id and marked
/// with the session it came from.
fn forked(from: &str, into: &str, line: &str) -> String {
    let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
    record["sessionId"] = serde_json::json!(into);
    record["forkedFrom"] = serde_json::json!({ "sessionId": from });
    serde_json::to_string(&record).unwrap()
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
        harness_paths: vec![".claude".into()],
    };
    (dir, config)
}

/// The record the runtime writes when the operator runs `/compact`, arguments
/// and all. It carries no prompt source — the operator typed the command, not
/// the wrapper — and the runtime writes it into the transcript *after* the
/// boundary it produced, holding the earlier timestamp it was typed at.
fn compact_command(session: &str, uuid: &str, ts: &str, args: &str) -> String {
    let text =
        format!("<command-name>/compact</command-name>\n<command-args>{args}</command-args>");
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","message":{{"content":{}}}}}"#,
        serde_json::to_string(&text).unwrap()
    )
}

/// The boundary the runtime writes once a compaction has run.
fn boundary(session: &str, uuid: &str, ts: &str, trigger: &str) -> String {
    format!(
        r#"{{"type":"system","subtype":"compact_boundary","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","compactMetadata":{{"trigger":"{trigger}","preTokens":900,"postTokens":90,"cumulativeDroppedTokens":810,"durationMs":10}}}}"#
    )
}

/// One turn of the agent's own output.
fn agent(session: &str, uuid: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","message":{{"id":"m_{uuid}","content":[{{"type":"text","text":"working"}}]}}}}"#
    )
}

/// An instruction the operator sent while the agent was still answering.
fn steering(session: &str, uuid: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","version":"2.1.246","origin":{{"kind":"human"}},"promptSource":"queued","message":{{"content":"no, the other one"}}}}"#
    )
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
    let blocks = &facts.prompts.across_sessions.as_ref().unwrap().blocks;
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
        harness_paths: vec![".claude".into()],
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
    assert!(
        withheld.prompts.across_sessions.as_ref().unwrap().blocks[0]
            .text
            .is_none()
    );
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
        asked.prompts.across_sessions.as_ref().unwrap().blocks[0]
            .text
            .as_deref(),
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
    assert!(session::require_coverage(&facts.coverage).is_ok());
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
fn a_session_window_keeps_the_subagents_that_session_dispatched() {
    let (_dir, config) = corpus(&[
        (
            "-Users-me-alpha/s1.jsonl",
            vec![
                typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
                edit("s1", "e1", "2026-08-01T09:05:00Z", "/repo/loader.rs"),
            ],
        ),
        (
            "-Users-me-alpha/s1/subagents/agent-1.jsonl",
            vec![edit(
                "s1",
                "g1",
                "2026-08-01T09:06:00Z",
                "/repo/exporter.rs",
            )],
        ),
        (
            "-Users-me-alpha/s2.jsonl",
            vec![
                typed("s2", "b1", "2026-08-01T09:10:00Z", ALSO_STANDING),
                edit("s2", "e2", "2026-08-01T09:11:00Z", "/repo/other.rs"),
            ],
        ),
    ]);
    let options = CollectOptions {
        session: Some("s1".into()),
        with_submissions: true,
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(facts.coverage.sessions, 1);
    assert_eq!(facts.submissions.len(), 1);
    assert_eq!(
        facts.submissions[0].written,
        [
            PathBuf::from("/repo/exporter.rs"),
            PathBuf::from("/repo/loader.rs")
        ],
        "the subagent's file carries the parent's id, so its work stays in the window"
    );
}

#[test]
fn a_subagent_edits_a_file_the_parent_committed_and_it_lands_under_that_commit() {
    let (_dir, config) = corpus(&[
        (
            "-Users-me-alpha/s1.jsonl",
            vec![
                typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
                commit("s1", "c1", "2026-08-01T09:10:00Z", "1111111"),
                commit("s1", "c2", "2026-08-01T09:40:00Z", "2222222"),
            ],
        ),
        (
            // The subagent's file carries no commit of its own, so read alone
            // it has nothing to place its edits against.
            "-Users-me-alpha/s1/subagents/agent-1.jsonl",
            vec![
                edit("s1", "g1", "2026-08-01T09:05:00Z", "/repo/loader.rs"),
                edit("s1", "g2", "2026-08-01T09:20:00Z", "/repo/loader.rs"),
            ],
        ),
    ]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.rework.commits, 2);
    assert_eq!(facts.rework.post_commit_reedits.len(), 1);
    let r = &facts.rework.post_commit_reedits[0];
    assert_eq!(r.commit, "1111111");
    assert_eq!(r.reedits, 1);
    assert_eq!(r.citations[0].uuid, "g2");
}

#[test]
fn an_edit_stamped_before_the_commit_it_was_written_after_is_still_rework() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            edit("s1", "e1", "2026-08-01T09:05:00Z", "/repo/loader.rs"),
            commit("s1", "c1", "2026-08-01T09:10:00Z", "1111111"),
            edit("s1", "e2", "2026-08-01T09:08:00Z", "/repo/loader.rs"),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(
        facts.rework.post_commit_reedits.len(),
        1,
        "the transcript is append-ordered; its timestamps are not"
    );
    assert_eq!(facts.rework.post_commit_reedits[0].citations[0].uuid, "e2");
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
        facts.prompts.within_sessions.blocks[0].submissions, 2,
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
            r#"{"type":"system","uuid":"x2","timestamp":"2026-08-01T09:00:02Z","sessionId":"s1","subtype":"local_command"}"#.to_string(),
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
            .get("system:local_command"),
        Some(&1)
    );
    assert_eq!(facts.coverage.records_malformed, 0);
}

/// A window that exercises every metric a baseline records, at values chosen so
/// no two are equal.
fn every_metric_corpus() -> (TempDir, SessionConfig) {
    let mut alpha = vec![
        typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
        spent("s1", "x1", "2026-08-01T09:00:01Z", "claude-opus-5", 700),
        rule_load(
            "s1",
            "r1",
            "2026-08-01T09:00:02Z",
            "/p/.claude/rules/a.md",
            "abcde",
        ),
        edit("s1", "e1", "2026-08-01T09:00:03Z", "/p/src/lib.rs"),
        commit("s1", "c1", "2026-08-01T09:00:04Z", "abc1234"),
        edit("s1", "e2", "2026-08-01T09:00:05Z", "/p/src/lib.rs"),
        stop_summary("s1", "h1", "2026-08-01T09:00:06Z", "check.sh", 90),
    ];
    alpha.extend(call_and_denial("s1", "Bash", "permission-rule", 7));
    alpha.push(spoke("s1", "x2", "2026-08-01T09:00:08Z"));
    // Queued after the agent spoke: a second instruction, and steering.
    alpha.push(queued("s1", "a2", "2026-08-01T09:00:09Z", STANDING));
    alpha.push(marked_interrupt("s1", "i1", "2026-08-01T09:00:10Z"));
    // Two in one session, so a running total summed twice is not the same
    // number as the session's own.
    alpha.push(compacted("s1", "k1", "2026-08-01T09:00:11Z", 900, 100, 100));
    alpha.push(compacted("s1", "k2", "2026-08-01T09:00:12Z", 800, 150, 250));

    let beta = vec![
        typed("s2", "b1", "2026-08-02T09:00:00Z", STANDING),
        compacted("s2", "k3", "2026-08-02T09:30:00Z", 500, 60, 40),
        typed("s2", "b2", "2026-08-02T10:00:00Z", STANDING),
    ];
    corpus(&[
        ("-Users-me-alpha/s1.jsonl", alpha),
        ("-Users-me-alpha/s2.jsonl", beta),
    ])
}

/// Every metric a baseline records, pinned to the window above.
///
/// A metric is compared against one an earlier build wrote, so a change to how
/// it is computed is a change to what a recorded number means. The module's
/// answer is to rename rather than redefine; this is what makes that a decision
/// somebody takes rather than one they can make by accident.
#[test]
fn every_recorded_metric_computes_what_it_computed() {
    let (_dir, config) = every_metric_corpus();
    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    let pinned = [
        ("cross_session_chars_per_session", 72, 2),
        ("within_session_chars_per_submission", 144, 4),
        ("rule_load_chars_per_submission", 5, 4),
        // 250 and 40, the running total each session reached — not 350, which
        // is what adding both of s1's boundaries would give.
        ("dropped_tokens_per_submission", 290, 4),
        ("denials_per_submission", 1, 4),
        ("steering_per_submission", 1, 4),
        ("interrupts_per_submission", 1, 4),
        ("reedits_per_commit", 1, 1),
        ("hook_milliseconds_per_stop", 90, 1),
        ("output_tokens_per_submission", 700, 4),
    ];
    assert_eq!(
        pinned.len(),
        session::SessionMetric::ALL.len(),
        "a metric was added without a value to hold it to"
    );

    for (name, numerator, denominator) in pinned {
        let metric = session::SessionMetric::from_str(name).expect("a metric by that name");
        assert_eq!(
            metric.measure(&facts),
            Some(session::Measurement {
                numerator,
                denominator
            }),
            "`{name}` no longer computes what it computed; rename it rather than redefining it"
        );
    }
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
    session::Baseline::of(
        session::Measured {
            label,
            recorded_at: "2026-09-01T00:00:00Z".parse().unwrap(),
            project: None,
            min_block_chars: config.min_block_chars,
            coverage_floor: config.coverage_floor,
            harness: None,
        },
        &facts,
    )
}

#[test]
fn a_fork_replaying_its_parent_does_not_make_one_instruction_into_two() {
    let original = typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING);
    let (_dir, config) = corpus(&[
        ("-Users-me-alpha/s1.jsonl", vec![original.clone()]),
        (
            "-Users-me-alpha/s2.jsonl",
            vec![
                forked("s1", "s2", &original),
                typed("s2", "b1", "2026-08-01T11:00:00Z", ALSO_STANDING),
            ],
        ),
    ]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(
        facts.prompts.submissions, 2,
        "the fork replayed one instruction and typed one"
    );
    assert_eq!(facts.coverage.records_forked, 1);
    assert_eq!(
        facts.coverage.sessions, 2,
        "the fork is a session; only its replayed records are not its events"
    );
}

#[test]
fn a_message_copied_into_another_transcript_under_new_uuids_is_one_message() {
    // The copies carry different uuids, so identity by record does not see
    // them. A message is written to one transcript; another reporting it is
    // reporting copies of its blocks, and its tool calls with them.
    let block = |uuid: &str, ts: &str| {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","sessionId":"s1","message":{{"id":"msg_a","model":"claude-opus-5","usage":{{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":400}},"content":[{{"type":"tool_use","id":"call-1","name":"Bash","input":{{"command":"true"}}}}]}}}}"#
        )
    };
    let (_dir, config) = corpus(&[
        (
            "-Users-me-alpha/s1.jsonl",
            vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
        ),
        (
            "-Users-me-alpha/s1/subagents/agent-one.jsonl",
            vec![block("x1", "2026-08-01T09:00:02Z")],
        ),
        (
            "-Users-me-alpha/s1/subagents/agent-two.jsonl",
            vec![block("x2", "2026-08-01T09:00:03Z")],
        ),
    ]);
    let options = CollectOptions {
        with_submissions: true,
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(facts.tokens.output, 400, "one message, charged once");
    assert_eq!(facts.tools["Bash"].calls, 1, "one tool call, counted once");
    assert_eq!(facts.coverage.records_duplicated, 1);
    assert_eq!(facts.submissions[0].agent_turns, 1);
}

#[test]
fn a_message_split_into_blocks_stays_every_block_it_produced() {
    // Two records of one message inside the file that wrote it are two events:
    // the message made two tool calls, and neither is a copy of the other.
    let block = |uuid: &str, ts: &str, call: &str, output: u64| {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","sessionId":"s1","message":{{"id":"msg_a","model":"claude-opus-5","usage":{{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":{output}}},"content":[{{"type":"tool_use","id":"{call}","name":"Bash","input":{{"command":"true"}}}}]}}}}"#
        )
    };
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            block("x1", "2026-08-01T09:00:02Z", "call-1", 4),
            block("x2", "2026-08-01T09:00:03Z", "call-2", 400),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.tools["Bash"].calls, 2);
    assert_eq!(
        facts.tokens.output, 400,
        "and one charge, at its settled count"
    );
    assert_eq!(facts.coverage.records_duplicated, 0);
}

#[test]
fn a_record_two_of_a_sessions_subagents_start_from_is_one_event() {
    let shared = spent("s1", "x1", "2026-08-01T09:00:02Z", "claude-opus-5", 400);
    let (_dir, config) = corpus(&[
        (
            "-Users-me-alpha/s1.jsonl",
            vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
        ),
        // Two subagents dispatched at once, each transcript opening with the
        // state they were handed, under the session that dispatched them.
        (
            "-Users-me-alpha/s1/subagents/agent-one.jsonl",
            vec![shared.clone()],
        ),
        ("-Users-me-alpha/s1/subagents/agent-two.jsonl", vec![shared]),
    ]);
    let options = CollectOptions {
        with_submissions: true,
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(
        facts.tokens.output, 400,
        "one turn was taken, and two files record it"
    );
    assert_eq!(facts.submissions[0].agent_turns, 1);
    assert_eq!(facts.coverage.records_duplicated, 1);
}

#[test]
fn every_window_verb_returns_something_that_says_which_window_it_is() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
    )]);
    let options = CollectOptions {
        with_submissions: true,
        ..CollectOptions::default()
    };
    let facts = session::collect(&config, &options).unwrap();

    // What each of `index`, `facts` and `submissions` puts in `data`. A saved
    // result that cannot say what it covers cannot be read beside another.
    let index = serde_json::to_value(&facts.coverage).unwrap();
    let full = serde_json::to_value(&facts).unwrap();
    let window = serde_json::to_value(session::SubmissionWindow {
        coverage: facts.coverage.clone(),
        submissions: facts.submissions.clone(),
    })
    .unwrap();

    for (verb, value) in [("facts", &full), ("submissions", &window)] {
        let coverage = value
            .get("coverage")
            .unwrap_or_else(|| panic!("`{verb}` carries the window it read"));
        assert!(coverage.get("observed_from").is_some());
        assert!(coverage.get("runtime_versions").is_some());
    }
    assert!(
        index.get("observed_from").is_some(),
        "`index` is the coverage"
    );
}

#[test]
fn every_analyser_sees_the_same_records_after_a_copy_is_discarded() {
    // The re-edit walk reads the group rather than one record at a time, so a
    // copy the other analysers discarded would reach it and count again.
    let edit = r#"{"type":"user","uuid":"e1","timestamp":"2026-08-01T09:00:20Z","sessionId":"s1","toolUseResult":{"filePath":"/w/alpha/src/lib.rs","structuredPatch":[]},"message":{"content":[{"type":"tool_result","content":"ok"}]}}"#.to_string();
    let (_dir, config) = corpus(&[
        (
            "-Users-me-alpha/s1.jsonl",
            vec![
                typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
                // Edited, shipped, then edited again — the shape a re-edit is.
                edit.replace("\"uuid\":\"e1\"", "\"uuid\":\"e0\"")
                    .replace("09:00:20", "09:00:05"),
                r#"{"type":"user","uuid":"c1","timestamp":"2026-08-01T09:00:10Z","sessionId":"s1","toolUseResult":{"gitOperation":{"commit":{"sha":"abc1234"}}},"message":{"content":[{"type":"tool_result","content":"ok"}]}}"#.to_string(),
                edit.clone(),
            ],
        ),
        ("-Users-me-alpha/s1/subagents/agent-one.jsonl", vec![edit]),
    ]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.coverage.records_duplicated, 1);
    assert_eq!(
        facts.rework.post_commit_reedits[0].reedits, 1,
        "one file was edited once after the commit, and two files recorded it"
    );
}

#[test]
fn a_scoped_window_says_how_many_files_it_drew_from_and_how_many_it_opened() {
    let (_dir, config) = corpus(&[
        (
            "-Users-me-alpha/s1.jsonl",
            vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
        ),
        (
            "-Users-me-beta/s2.jsonl",
            vec![typed("s2", "b1", "2026-08-01T10:00:00Z", ALSO_STANDING)],
        ),
    ]);
    let options = CollectOptions {
        session: Some("s1".into()),
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(facts.coverage.files_discovered, 2, "what it cost to answer");
    assert_eq!(facts.coverage.files_in_window, 1, "what it answered about");
}

#[test]
fn a_tool_result_is_not_a_turn_the_runtime_declined_to_attribute() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            unclaimed(
                "s1",
                "u1",
                "2026-08-01T09:00:05Z",
                "[Request interrupted by user]",
            ),
            r#"{"type":"user","uuid":"t1","timestamp":"2026-08-01T09:00:06Z","sessionId":"s1","message":{"content":[{"type":"tool_result","content":"ok"}]}}"#.to_string(),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(
        facts.coverage.user_turns_by_authorship[Authorship::Unclaimed.as_str()],
        1,
        "the interrupt is a turn nobody claimed; the tool result is not a turn"
    );
}

#[test]
fn a_turn_replayed_into_a_second_session_is_not_a_paragraph_written_twice() {
    // A resumed session can carry the earlier one's records verbatim under a
    // new id and without the fork marker. Two sessions then hold one turn, and
    // a paragraph typed once looks like one no harness was holding.
    let original = typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING);
    let replayed = original.replace("\"sessionId\":\"s1\"", "\"sessionId\":\"s2\"");
    let (_dir, config) = corpus(&[
        ("-Users-me-alpha/s1.jsonl", vec![original]),
        (
            "-Users-me-alpha/s2.jsonl",
            vec![
                replayed,
                typed("s2", "b1", "2026-08-01T11:00:00Z", ALSO_STANDING),
            ],
        ),
    ]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.coverage.records_duplicated, 1);
    assert_eq!(facts.prompts.submissions, 2);
    assert_eq!(
        facts.prompts.across_sessions.as_ref().unwrap().chars,
        0,
        "one turn reached the reader twice; the operator typed it once"
    );
}

#[test]
fn a_window_scoped_to_a_fork_does_not_inherit_what_it_was_forked_from() {
    let original = typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING);
    let (_dir, config) = corpus(&[
        ("-Users-me-alpha/s1.jsonl", vec![original.clone()]),
        (
            "-Users-me-alpha/s2.jsonl",
            vec![
                forked("s1", "s2", &original),
                typed("s2", "b1", "2026-08-01T11:00:00Z", ALSO_STANDING),
            ],
        ),
    ]);
    let options = CollectOptions {
        session: Some("s2".into()),
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(
        facts.prompts.submissions, 1,
        "the replayed instruction was given to the session this one came from"
    );
}

#[test]
fn a_metric_the_window_could_not_measure_is_absent_rather_than_zero() {
    let (dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            typed("s1", "a2", "2026-08-01T10:00:00Z", STANDING),
        ],
    )]);
    const ACROSS: &str = "cross_session_chars_per_session";

    let one_session = baseline_of(&config, None, "one");
    assert!(
        !one_session.measurements.contains_key(ACROSS),
        "a paragraph cannot cross into a session the window does not hold"
    );
    assert!(
        one_session
            .measurements
            .contains_key("within_session_chars_per_submission"),
        "the question this window can answer is still answered"
    );

    std::fs::write(
        dir.path().join("-Users-me-alpha/s2.jsonl"),
        [typed("s2", "b1", "2026-08-05T09:00:00Z", STANDING)].join("\n"),
    )
    .unwrap();
    let two_sessions = baseline_of(&config, None, "two");
    assert!(two_sessions.measurements.contains_key(ACROSS));

    let diff = session::baseline::diff(&one_session, &two_sessions, config.min_support);
    let unmatched = diff.expect_err("the windows overlap");
    assert_eq!(unmatched.code(), ErrorCode::SessionBaselineNotComparable);
}

#[test]
fn a_record_on_the_boundary_belongs_to_one_of_the_two_windows() {
    // Both turns land on the same instant, which 5.55% of adjacent records do.
    // Resuming from that instant would put them in the earlier window and the
    // later one, and a comparison would call the pair consecutive.
    let (dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            typed("s1", "a2", "2026-08-01T10:00:00Z", STANDING),
        ],
    )]);
    let ledger = session::BaselineLedger::new(config.baseline_path.clone());
    ledger
        .append(&baseline_of(&config, None, "before"))
        .unwrap();

    std::fs::write(
        dir.path().join("-Users-me-alpha/s2.jsonl"),
        [typed("s2", "b1", "2026-08-01T10:00:00Z", ALSO_STANDING)].join("\n"),
    )
    .unwrap();

    let resume = session::baseline::latest_observed_to(&ledger.load_all().unwrap(), None).unwrap();
    let after = baseline_of(&config, Some(&resume.to_string()), "after");
    ledger.append(&after).unwrap();

    let recorded = ledger.load_all().unwrap();
    let (from, to) = session::baseline::select(&recorded, None, None).unwrap();
    assert_eq!(
        from.measurements["denials_per_submission"].denominator, 2,
        "the window that stopped at the boundary measured what was on it"
    );
    assert_eq!(
        to.measurements["denials_per_submission"].denominator, 0,
        "and the window resuming after it measured none of the same records"
    );
}

#[test]
fn trend_lays_only_one_scope_side_by_side_in_ledger_order() {
    let (_dir, config) = every_metric_corpus();
    let first = baseline_of(&config, None, "first");
    let second = baseline_of(&config, None, "second");
    let mut scoped = baseline_of(&config, None, "scoped");
    scoped.project = Some(PathBuf::from("/p"));
    let ledger = vec![first, second, scoped];

    let labels = |trend: &session::BaselineTrend| -> Vec<String> {
        trend.windows.iter().map(|w| w.label.clone()).collect()
    };

    let unscoped = session::baseline::trend(&ledger, None);
    assert_eq!(labels(&unscoped), ["first", "second"]);
    let steering = unscoped
        .series
        .iter()
        .find(|s| s.metric == "steering_per_submission")
        .expect("a series per recorded metric");
    assert_eq!(
        steering
            .points
            .iter()
            .map(|p| p.label.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"],
        "every window of the scope contributes its point, in ledger order"
    );

    let scoped = session::baseline::trend(&ledger, Some(std::path::Path::new("/p")));
    assert_eq!(
        labels(&scoped),
        ["scoped"],
        "a series mixing scopes would trend the scope"
    );
}

#[test]
fn trend_omits_a_metric_a_window_could_not_measure() {
    let (_dir, config) = every_metric_corpus();
    let first = baseline_of(&config, None, "first");
    let mut second = baseline_of(&config, None, "second");
    second.measurements.remove("reedits_per_commit");
    let trend = session::baseline::trend(&[first, second], None);

    let reedits = trend
        .series
        .iter()
        .find(|s| s.metric == "reedits_per_commit")
        .expect("the metric one window carries still has a series");
    assert_eq!(
        reedits
            .points
            .iter()
            .map(|p| p.label.as_str())
            .collect::<Vec<_>>(),
        ["first"],
        "a window that could not measure a metric is absent from it, never zero"
    );
}

fn baseline_under(
    config: &SessionConfig,
    label: &str,
    harness: Option<session::HarnessState>,
) -> session::Baseline {
    let facts = session::collect(config, &CollectOptions::default()).unwrap();
    session::Baseline::of(
        session::Measured {
            label,
            recorded_at: "2026-09-01T00:00:00Z".parse().unwrap(),
            project: Some("/w/alpha".into()),
            min_block_chars: config.min_block_chars,
            coverage_floor: config.coverage_floor,
            harness,
        },
        &facts,
    )
}

/// A metric can become wrong without its definition moving — a build that
/// counted a record twice reported an honest name over a dishonest set — so a
/// comparison says whether the ruler was the same before it says anything
/// about the work.
#[test]
fn a_comparison_says_whether_the_two_windows_were_measured_the_same_way() {
    let (dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
    )]);
    let before = baseline_under(&config, "before", None);
    std::fs::write(
        dir.path().join("-Users-me-alpha/s2.jsonl"),
        [typed("s2", "b1", "2026-08-09T09:00:00Z", ALSO_STANDING)].join("\n"),
    )
    .unwrap();
    let after = baseline_under(&config, "after", None);

    let change = |a: &session::Baseline, b: &session::Baseline| {
        session::baseline::diff(a, b, config.min_support)
            .unwrap()
            .method_change
    };
    assert_eq!(change(&before, &after), "unchanged", "one build, one floor");

    let mut other_build = after.clone();
    other_build.oracle_version = "0.0.1-other".into();
    assert_eq!(change(&before, &other_build), "changed");

    let mut other_floor = after.clone();
    other_floor.min_block_chars = config.min_block_chars + 1;
    assert_eq!(
        change(&before, &other_floor),
        "changed",
        "the paragraph floor decides what the repetition metrics counted"
    );

    let mut other_coverage = after.clone();
    other_coverage.coverage_floor = Some(config.coverage_floor / 2.0);
    assert_eq!(
        change(&before, &other_coverage),
        "changed",
        "the coverage floor decides whether the operator-text metrics exist"
    );

    let mut unrecorded = after.clone();
    unrecorded.coverage_floor = None;
    assert_eq!(
        change(&before, &unrecorded),
        "unknown",
        "a window that did not record its coverage floor answers neither way"
    );

    // Both halves of what is known, one at a time: a mismatch either side of
    // the pair answers before the unrecorded floor is reached.
    let mut unrecorded_and_rebuilt = unrecorded.clone();
    unrecorded_and_rebuilt.oracle_version = "0.0.1-other".into();
    assert_eq!(
        change(&before, &unrecorded_and_rebuilt),
        "changed",
        "a build that moved is answered even where the floor is unrecorded"
    );

    let mut unrecorded_and_refloored = unrecorded.clone();
    unrecorded_and_refloored.min_block_chars = config.min_block_chars + 1;
    assert_eq!(
        change(&before, &unrecorded_and_refloored),
        "changed",
        "a paragraph floor that moved is answered even where the floor is unrecorded"
    );

    // A verdict a reader cannot open is one they have to take on trust, so
    // each side reports the floor it was measured under beside the ratio that
    // was judged against it.
    let reported = session::baseline::diff(&before, &unrecorded, config.min_support).unwrap();
    assert_eq!(reported.from.coverage_floor, Some(config.coverage_floor));
    assert_eq!(reported.to.coverage_floor, None);
}

/// A ledger written before a window recorded what measured it holds numbers no
/// comparison can place, so it is refused at the line rather than read as a
/// window that happens to agree.
#[test]
fn a_ledger_that_predates_what_measured_it_is_refused_by_the_line() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("baselines.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"label":"old","recorded_at":"2026-08-01T00:00:00Z","#,
            r#""coverage":{"files_discovered":1,"files_read":1,"files_unreadable":0,"#,
            r#""records_total":1,"records_malformed":0,"record_types_unconsumed":{},"#,
            r#""user_turns_by_authorship":{},"runtime_versions":[],"models":[],"sessions":1},"#,
            r#""measurements":{}}"#,
            "\n"
        ),
    )
    .unwrap();

    let err = session::BaselineLedger::new(path.clone())
        .load_all()
        .expect_err("a row missing what measured it is not a baseline");
    assert_eq!(
        err.code(),
        harness_core::error::ErrorCode::SessionBaselineUnreadable,
        "the file read; the row is not a window"
    );
    let rendered = err.to_string();
    assert!(
        rendered.contains("baseline line 1"),
        "the refusal names the line to open: {rendered}"
    );
    assert!(err.hint().is_some(), "and what to do about it");
}

#[test]
fn a_comparison_says_whether_the_harness_moved_between_the_two_windows() {
    let (dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
    )]);
    let at = |sha: &str, uncommitted: bool| {
        Some(session::HarnessState {
            head: Some(sha.into()),
            uncommitted,
        })
    };
    let later = |label: &str, harness| {
        std::fs::write(
            dir.path().join("-Users-me-alpha/s2.jsonl"),
            [typed("s2", "b1", "2026-08-09T09:00:00Z", ALSO_STANDING)].join("\n"),
        )
        .unwrap();
        baseline_under(&config, label, harness)
    };

    let before = baseline_under(&config, "before", at("aaa", false));
    let same = later("same", at("aaa", false));
    let moved = later("moved", at("bbb", false));
    let dirty = later("dirty", at("aaa", true));
    let absent = later("absent", None);

    let change = |a: &session::Baseline, b: &session::Baseline| {
        session::baseline::diff(a, b, config.min_support)
            .unwrap()
            .harness_change
    };

    assert_eq!(change(&before, &same), "unchanged");
    assert_eq!(change(&before, &moved), "changed");
    assert_eq!(
        change(&before, &dirty),
        "unknown",
        "the same commit, and one window did not run what it names"
    );
    assert_eq!(change(&before, &absent), "unknown");
}

#[test]
fn a_baseline_records_the_build_that_measured_it() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
    )]);

    let recorded = baseline_of(&config, None, "one");

    assert_eq!(
        recorded.oracle_version,
        env!("CARGO_PKG_VERSION"),
        "a metric whose definition moved between builds is a delta about the build"
    );
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
        .find(|m| m.metric == "within_session_chars_per_submission")
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
fn a_window_names_the_rates_no_comparison_against_it_will_answer() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)],
    )]);
    let baseline = baseline_of(&config, None, "thin");

    assert_eq!(
        baseline.unsupported(30).len(),
        baseline.measurements.len(),
        "one instruction supports no rate at a floor of thirty"
    );
    assert!(
        baseline
            .unsupported(30)
            .contains(&"output_tokens_per_submission"),
        "the metrics are named, so a consumer can say which"
    );
    assert_eq!(
        baseline.unsupported(0),
        ["hook_milliseconds_per_stop", "reedits_per_commit"],
        "a rate over an empty population is withheld at any floor, zero included"
    );
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
        .find(|m| m.metric == "within_session_chars_per_submission")
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

    assert_eq!(err.code(), ErrorCode::SessionBaselineUnreadable);
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
    spent(session, uuid, ts, "claude-sonnet-5", 0)
}

/// An agent turn that produced text and reported what it cost.
fn spent(session: &str, uuid: &str, ts: &str, model: &str, output: u64) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","message":{{"model":"{model}","usage":{{"input_tokens":7,"cache_creation_input_tokens":11,"cache_read_input_tokens":13,"output_tokens":{output}}},"content":[{{"type":"text","text":"working on it"}}]}}}}"#
    )
}

/// One record of an assistant message, as the runtime writes them: a record per
/// content block, each repeating the message's usage.
fn block_of(session: &str, uuid: &str, ts: &str, id: &str, output: u64) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"input_tokens":7,"cache_creation_input_tokens":11,"cache_read_input_tokens":13,"output_tokens":{output}}},"content":[{{"type":"text","text":"working on it"}}]}}}}"#
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
    assert_eq!((first.edits, first.commits.len()), (3, 1));
    assert_eq!(
        first.written,
        [
            PathBuf::from("/p/src/exporter.rs"),
            PathBuf::from("/p/src/loader.rs")
        ],
        "three edits over two files, each named once and in path order"
    );
    assert_eq!(first.commits, vec!["abc1234"]);
    assert_eq!(first.questions, 1);
    assert_eq!(
        (
            second.edits,
            second.written.as_slice(),
            second.commits.len()
        ),
        (1, [PathBuf::from("/p/src/loader.rs")].as_slice(), 0),
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
        session::Measured {
            label: "scoped",
            recorded_at: "2026-09-01T00:00:00Z".parse().unwrap(),
            project: Some("/w/alpha".into()),
            min_block_chars: config.min_block_chars,
            coverage_floor: config.coverage_floor,
            harness: None,
        },
        &facts,
    );

    let err = session::baseline::diff(&whole, &scoped, config.min_support).unwrap_err();

    assert_eq!(err.code(), ErrorCode::SessionBaselineNotComparable);
}

/// An agent turn that invoked a harness element.
fn invoked(session: &str, uuid: &str, ts: &str, tool: &str, key: &str, name: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","message":{{"content":[{{"type":"tool_use","id":"i{uuid}","name":"{tool}","input":{{"{key}":"{name}"}}}}]}}}}"#
    )
}

#[test]
fn the_same_call_refused_twice_is_one_row_and_its_text_is_opt_in() {
    let mut lines = vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)];
    for second in [11, 12, 13] {
        lines.extend(call_and_denial("s1", "Bash", "permission-rule", second));
    }
    let (_dir, config) = corpus(&[("-Users-me-alpha/s1.jsonl", lines)]);

    let quiet = session::collect(&config, &CollectOptions::default()).unwrap();
    let spoken = session::collect(
        &config,
        &CollectOptions {
            with_text: true,
            ..CollectOptions::default()
        },
    )
    .unwrap();

    assert_eq!(quiet.harness.blocked.len(), 1, "one call, three attempts");
    assert_eq!(quiet.harness.blocked[0].attempts, 3);
    assert_eq!(quiet.harness.blocked[0].tool.as_deref(), Some("Bash"));
    assert!(quiet.harness.blocked[0].input.is_none());
    assert_eq!(
        spoken.harness.blocked[0].input.as_ref().unwrap()["command"],
        "x"
    );
}

#[test]
fn a_sub_agent_counts_the_same_however_the_runtime_named_its_tool() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            invoked(
                "s1",
                "x1",
                "2026-08-01T09:00:01Z",
                "Task",
                "subagent_type",
                "reviewer",
            ),
            invoked(
                "s1",
                "x2",
                "2026-08-01T09:00:02Z",
                "Agent",
                "subagent_type",
                "reviewer",
            ),
            invoked(
                "s1",
                "x3",
                "2026-08-01T09:00:03Z",
                "Skill",
                "skill",
                "harnex",
            ),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    let by_name: Vec<_> = facts
        .harness
        .invocations
        .iter()
        .map(|i| (i.kind.as_str(), i.name.as_str(), i.calls))
        .collect();
    assert_eq!(
        by_name,
        vec![("agent", "reviewer", 2), ("skill", "harnex", 1)]
    );
}

/// The system record marking where the context was compacted.
fn compacted(session: &str, uuid: &str, ts: &str, pre: u64, post: u64, cumulative: u64) -> String {
    format!(
        r#"{{"type":"system","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","subtype":"compact_boundary","compactMetadata":{{"trigger":"manual","preTokens":{pre},"postTokens":{post},"cumulativeDroppedTokens":{cumulative},"durationMs":1200}}}}"#
    )
}

#[test]
fn a_compaction_is_read_as_an_event_and_not_as_an_unread_record_type() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            compacted("s1", "k1", "2026-08-01T10:00:00Z", 754_436, 15_645, 738_791),
            compacted(
                "s1",
                "k2",
                "2026-08-01T11:00:00Z",
                700_000,
                12_000,
                1_426_791,
            ),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.compactions.len(), 2);
    assert_eq!(facts.compactions[0].trigger, "manual");
    assert_eq!(facts.compactions[0].pre_tokens, 754_436);
    assert_eq!(
        facts.compactions[1].cumulative_dropped_tokens, 1_426_791,
        "the runtime's running total, not this event's drop"
    );
    assert!(
        !facts
            .coverage
            .record_types_unconsumed
            .contains_key("system:compact_boundary")
    );
}

#[test]
fn an_instruction_carries_what_it_spent_and_which_models_spent_it() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            spent("s1", "x1", "2026-08-01T09:00:02Z", "claude-opus-5", 400),
            spent("s1", "x2", "2026-08-01T09:00:04Z", "claude-sonnet-5", 100),
            typed("s1", "a2", "2026-08-01T10:00:00Z", ALSO_STANDING),
            spent("s1", "x3", "2026-08-01T10:00:02Z", "claude-opus-5", 60),
        ],
    )]);
    let options = CollectOptions {
        with_submissions: true,
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(facts.tokens.output, 560, "the window total is every turn");
    assert_eq!(facts.submissions[0].tokens.output, 500);
    assert_eq!(facts.submissions[0].tokens.cache_read, 26);
    assert_eq!(
        facts.submissions[0].models,
        vec!["claude-opus-5", "claude-sonnet-5"],
        "two models answered one instruction, and the record says so"
    );
    assert_eq!(facts.submissions[1].tokens.output, 60);
    assert_eq!(
        facts.coverage.models.len(),
        2,
        "the window's model set rides with its version set"
    );
}

#[test]
fn a_turn_that_spent_without_naming_its_message_is_charged_and_reported() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            // The shape before the runtime named its messages: usage, no id.
            // Nothing can hold the charge against a message, so it is taken as
            // new — which is the pre-fix behaviour, and the counter is how a
            // reader learns the totals are a ceiling again.
            spent("s1", "x1", "2026-08-01T09:00:02Z", "claude-opus-5", 400),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.tokens.output, 400);
    assert_eq!(facts.coverage.turns_charged_without_a_message, 1);
}

#[test]
fn a_message_split_across_records_is_charged_once_at_its_settled_count() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            // One message, three records: two written while it was still
            // streaming and one after, all repeating the same usage.
            block_of("s1", "x1", "2026-08-01T09:00:02Z", "msg_a", 4),
            block_of("s1", "x2", "2026-08-01T09:00:03Z", "msg_a", 4),
            block_of("s1", "x3", "2026-08-01T09:00:04Z", "msg_a", 400),
            block_of("s1", "x4", "2026-08-01T09:00:06Z", "msg_b", 100),
        ],
    )]);
    let options = CollectOptions {
        with_submissions: true,
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(
        facts.tokens.output, 500,
        "two messages were answered, not four"
    );
    assert_eq!(
        facts.tokens.cache_read, 26,
        "a repeated usage is the same read, not another one"
    );
    assert_eq!(facts.submissions[0].tokens.output, 500);
    assert_eq!(facts.coverage.turns_charged_without_a_message, 0);
}

#[test]
fn the_tool_mix_is_counted_per_window_and_per_instruction() {
    let mut lines = vec![typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING)];
    lines.extend(call_and_denial("s1", "Bash", "permission-rule", 11));
    lines.extend(call_and_denial("s1", "Bash", "permission-rule", 12));
    lines.push(invoked(
        "s1",
        "x1",
        "2026-08-01T10:00:13Z",
        "Skill",
        "skill",
        "harnex",
    ));
    lines.push(typed("s2", "b1", "2026-08-02T09:00:00Z", ALSO_STANDING));
    let (_dir, config) = corpus(&[("-Users-me-alpha/s1.jsonl", lines)]);
    let options = CollectOptions {
        with_submissions: true,
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(facts.tools["Bash"].calls, 2);
    assert_eq!(facts.tools["Skill"].calls, 1);
    assert_eq!(facts.submissions[0].tools["Bash"].calls, 2);
    assert!(facts.submissions[1].tools.is_empty());
    assert_eq!(facts.coverage.sessions, 2);
}

/// A turn the operator typed with a file dropped in beside the text.
fn typed_with_image(session: &str, uuid: &str, ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","version":"2.1.246","origin":{{"kind":"human"}},"promptSource":"typed","message":{{"content":[{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"iVBOR"}}}},{{"type":"text","text":{}}}]}}}}"#,
        serde_json::to_string(text).unwrap()
    )
}

#[test]
fn an_instruction_with_a_file_beside_it_is_still_an_instruction() {
    let mut lines = vec![
        typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
        typed_with_image("s1", "a2", "2026-08-01T10:00:00Z", ALSO_STANDING),
    ];
    lines.extend(call_and_denial("s1", "Bash", "permission-rule", 11));
    let (_dir, config) = corpus(&[("-Users-me-alpha/s1.jsonl", lines)]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(
        facts.prompts.authored_turns,
        facts.coverage.user_turns_by_authorship[Authorship::Authored.as_str()],
        "one envelope, one meaning for `authored`"
    );
    assert_eq!(facts.prompts.submissions, 2);
    assert_eq!(
        facts.harness.denials.len(),
        1,
        "a tool result still carries no text and opens no instruction"
    );
}

#[test]
fn a_subagents_work_lands_on_the_instruction_that_was_standing() {
    let (_dir, config) = corpus(&[
        (
            "-Users-me-alpha/s1.jsonl",
            vec![
                typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
                spent("s1", "x1", "2026-08-01T09:00:10Z", "claude-opus-5", 100),
                typed("s1", "a2", "2026-08-01T11:00:00Z", ALSO_STANDING),
                spent("s1", "x2", "2026-08-01T11:00:10Z", "claude-opus-5", 100),
            ],
        ),
        (
            // A subagent writes its own file under the parent's session and
            // carries the parent's id. Ran under the first instruction.
            "-Users-me-alpha/s1/subagents/agent-1.jsonl",
            vec![
                spent("s1", "g1", "2026-08-01T09:30:00Z", "claude-sonnet-5", 400),
                spent("s1", "g2", "2026-08-01T09:40:00Z", "claude-sonnet-5", 300),
            ],
        ),
    ]);
    let options = CollectOptions {
        with_submissions: true,
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    let attributed: u64 = facts.submissions.iter().map(|s| s.tokens.output).sum();
    assert_eq!(
        attributed, facts.tokens.output,
        "every token the window spent belongs to an instruction"
    );
    assert_eq!(facts.submissions[0].tokens.output, 800);
    assert_eq!(facts.submissions[0].agent_turns, 3);
    assert_eq!(facts.submissions[1].tokens.output, 100);
    assert_eq!(
        facts.submissions[0].models,
        vec!["claude-opus-5", "claude-sonnet-5"]
    );
}

#[test]
fn a_groups_span_is_its_earliest_and_latest_whatever_order_the_files_arrive_in() {
    let (_dir, config) = corpus(&[
        (
            "-Users-me-alpha/zzz.jsonl",
            vec![
                typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
                rule_load("s1", "r1", "2026-08-01T09:00:01Z", "/p/CLAUDE.md", "old"),
            ],
        ),
        (
            "-Users-me-alpha/aaa.jsonl",
            vec![
                typed("s2", "b1", "2026-08-20T09:00:00Z", STANDING),
                rule_load("s2", "r2", "2026-08-20T09:00:01Z", "/p/CLAUDE.md", "new"),
            ],
        ),
    ]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    let span = &facts.harness.rule_loads[0].span;
    assert!(
        span.first.timestamp < span.last.timestamp,
        "path order put the later file first; the span is by time"
    );
    assert_eq!(span.first.uuid, "r1");
    assert_eq!(span.last.uuid, "r2");
}

#[test]
fn a_paragraph_that_is_both_failures_is_counted_as_both() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            spoke("s1", "x1", "2026-08-01T09:00:05Z"),
            typed("s1", "a2", "2026-08-01T10:00:00Z", STANDING),
            typed("s2", "b1", "2026-08-02T09:00:00Z", STANDING),
            spoke("s2", "y1", "2026-08-02T09:00:05Z"),
            typed("s2", "b2", "2026-08-02T10:00:00Z", STANDING),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();
    let chars = STANDING.chars().count();

    assert_eq!(facts.prompts.submissions, 4);
    assert_eq!(
        facts.prompts.across_sessions.as_ref().unwrap().chars,
        chars,
        "one session beyond the first"
    );
    assert_eq!(
        facts.prompts.within_sessions.chars,
        chars * 2,
        "one restatement inside each of the two sessions"
    );
    assert_eq!(
        facts.prompts.across_sessions.as_ref().unwrap().blocks.len(),
        1
    );
    assert_eq!(
        facts.prompts.within_sessions.blocks.len(),
        1,
        "the same paragraph is in both lists because it wants both fixes"
    );
}

/// A refused call never ran, and the runtime flags its result an error too, so
/// counting the flag alone reports the harness's refusals as the tool's
/// failures.
#[test]
fn a_call_the_harness_refused_is_not_a_call_that_failed() {
    let call = |uuid: &str, ts: &str, id: &str| {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","sessionId":"s1","message":{{"id":"m_{uuid}","content":[{{"type":"tool_use","id":"{id}","name":"Bash","input":{{}}}}]}}}}"#
        )
    };
    let result = |uuid: &str, ts: &str, id: &str, denial: &str| {
        format!(
            r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"s1"{denial},"message":{{"content":[{{"type":"tool_result","tool_use_id":"{id}","is_error":true,"content":"no"}}]}}}}"#
        )
    };
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            call("c1", "2026-08-01T09:00:01Z", "t1"),
            result("r1", "2026-08-01T09:00:02Z", "t1", ""),
            call("c2", "2026-08-01T09:00:03Z", "t2"),
            result(
                "r2",
                "2026-08-01T09:00:04Z",
                "t2",
                r#","toolDenialKind":"permission-rule""#,
            ),
        ],
    )]);

    let options = CollectOptions {
        with_submissions: true,
        ..CollectOptions::default()
    };

    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(facts.tools["Bash"].calls, 2, "both calls were made");
    assert_eq!(
        facts.tools["Bash"].failed, 1,
        "one ran and failed; the other was refused and never ran"
    );
    assert_eq!(
        facts
            .harness
            .denials
            .iter()
            .map(|d| d.denials)
            .sum::<usize>(),
        1,
        "and the refusal is counted where refusals are counted"
    );

    let one = &facts.submissions[0];
    assert_eq!(
        one.tools["Bash"].calls, 2,
        "the instruction made both calls"
    );
    assert_eq!(
        one.tools["Bash"].failed, 1,
        "the instruction that made the call carries its failure, as the window does"
    );
    assert_eq!(one.denials, 1, "and its refusal stays a refusal");
}

/// A prompt the operator chose rather than typed is still an instruction: the
/// runtime attributes it to a person, and the work under it is theirs. Its text
/// is not theirs, so it stays out of the repetition statistics.
#[test]
fn an_instruction_the_operator_chose_rather_than_typed_is_still_an_instruction() {
    let chosen = |uuid: &str, ts: &str, text: &str| {
        format!(
            r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"s1","origin":{{"kind":"human"}},"promptSource":"suggestion_accepted","message":{{"content":"{text}"}}}}"#
        )
    };
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            spent("s1", "x1", "2026-08-01T09:00:01Z", "claude-opus-5", 100),
            chosen("a2", "2026-08-01T09:00:02Z", STANDING),
            spent("s1", "x2", "2026-08-01T09:00:03Z", "claude-opus-5", 900),
        ],
    )]);

    let facts = session::collect(
        &config,
        &CollectOptions {
            with_text: true,
            ..CollectOptions::default()
        },
    )
    .unwrap();

    assert_eq!(
        facts.prompts.submissions, 2,
        "the second instruction opened its own, or its work is attributed to the first"
    );
    assert_eq!(
        facts.prompts.authored_turns, 1,
        "and only the typed one is text the operator wrote"
    );
    assert_eq!(
        facts.prompts.within_sessions.chars, 0,
        "so a paragraph the operator did not type is not a paragraph they wrote twice"
    );
    assert_eq!(
        facts.coverage.user_turns_by_authorship["source-unrecognised"], 1,
        "coverage still says the source was one this binary does not recognise"
    );
}

/// An instruction's span ends at the last record made under it, so time the
/// operator spent before sending the next one belongs to nobody. The runtime's
/// own `turn_duration` measures stop-to-stop and would have charged it here.
#[test]
fn an_instruction_is_as_long_as_its_work_and_not_as_long_as_the_wait_after_it() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            spent("s1", "x1", "2026-08-01T09:00:30Z", "claude-opus-5", 100),
            // The run ends, and three hours pass with the session open. The
            // stop and the operator's next message both land inside the first
            // instruction's interval and neither is it working.
            stop_summary("s1", "h1", "2026-08-01T09:00:31Z", "check.sh", 5),
            typed("s1", "a2", "2026-08-01T12:00:00Z", ALSO_STANDING),
            spent("s1", "x2", "2026-08-01T12:00:10Z", "claude-opus-5", 100),
        ],
    )]);

    let facts = session::collect(
        &config,
        &CollectOptions {
            with_submissions: true,
            ..CollectOptions::default()
        },
    )
    .unwrap();
    let subs = facts.submissions;

    assert_eq!(subs[0].elapsed_ms, 30_000, "half a minute of work");
    assert_eq!(subs[1].elapsed_ms, 10_000);
    assert!(
        subs.iter().all(|s| s.elapsed_ms < 3_600_000),
        "the three idle hours between them are in neither"
    );
}

/// The coverage floor describes how much of the operator's writing was read, so
/// it decides the rates taken over that writing and nothing else.
#[test]
fn a_window_that_read_some_of_the_writing_still_answers_about_everything_else() {
    let odd = |uuid: &str, ts: &str| {
        format!(
            r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"s1","origin":{{"kind":"human"}},"promptSource":"shipped-tomorrow","message":{{"content":"{STANDING}"}}}}"#
        )
    };
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            odd("a2", "2026-08-01T09:00:01Z"),
            stop_summary("s1", "h1", "2026-08-01T09:00:02Z", "check.sh", 90),
        ],
    )]);
    let facts = session::collect(&config, &CollectOptions::default()).unwrap();
    assert_eq!(facts.coverage.authorship_ratio(), Some(0.5));

    let recorded = session::Baseline::of(
        session::Measured {
            label: "half-read",
            recorded_at: "2026-09-01T00:00:00Z".parse().unwrap(),
            project: None,
            min_block_chars: config.min_block_chars,
            coverage_floor: config.coverage_floor,
            harness: None,
        },
        &facts,
    );

    assert!(
        recorded
            .measurements
            .contains_key("hook_milliseconds_per_stop"),
        "a hook's wall-clock does not depend on a prompt source this binary has not heard of"
    );
    for text_rate in [
        "cross_session_chars_per_session",
        "within_session_chars_per_submission",
    ] {
        assert!(
            !recorded.measurements.contains_key(text_rate),
            "`{text_rate}` is taken over writing the window only partly read"
        );
    }
}

/// A boundary is a `system` record carrying no text of its own, so what the
/// operator asked the compaction to keep is read from the `/compact` record
/// that precedes it. Whether they asked is a count anyone may read; what they
/// asked is their own writing, and travels under the same gate as a prompt.
#[test]
fn what_a_compaction_was_asked_to_keep_is_read_from_the_command_that_asked_it() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            boundary("s1", "b1", "2026-08-01T09:01:01Z", "manual"),
            compact_command("s1", "k1", "2026-08-01T09:01:00Z", "keep the plan"),
            typed("s1", "a2", "2026-08-01T09:02:00Z", ALSO_STANDING),
            boundary("s1", "b2", "2026-08-01T09:03:00Z", "auto"),
        ],
    )]);

    let withheld = session::collect(&config, &CollectOptions::default()).unwrap();
    let asked = &withheld.compactions[0];
    let alone = &withheld.compactions[1];

    assert_eq!(
        asked.instruction_chars,
        Some("keep the plan".chars().count()),
        "the operator asked for something, and how much is not their writing"
    );
    assert_eq!(
        asked.instruction, None,
        "what they asked is withheld until a caller asks for text"
    );
    assert_eq!(
        alone.instruction_chars, None,
        "the runtime compacted on its own, which is not an empty instruction"
    );

    let options = CollectOptions {
        with_text: true,
        ..CollectOptions::default()
    };
    let told = session::collect(&config, &options).unwrap();
    assert_eq!(
        told.compactions[0].instruction.as_deref(),
        Some("keep the plan"),
        "and it is there for a caller that asked"
    );
}

/// The turns a compaction's summary has to carry alone run to the next
/// instruction. An intervention is the operator correcting that run rather than
/// starting new work — it is the cost this span exists to measure, so it stays
/// inside the window instead of ending it. The turns outside are reported
/// beside it, because a recovery rate read without them says the opposite of
/// what it means.
#[test]
fn a_compaction_is_charged_until_the_next_instruction_and_being_corrected_is_not_one() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            agent("s1", "t1", "2026-08-01T09:00:10Z"),
            boundary("s1", "b1", "2026-08-01T09:01:01Z", "manual"),
            compact_command("s1", "k1", "2026-08-01T09:01:00Z", "keep going"),
            agent("s1", "t2", "2026-08-01T09:01:10Z"),
            steering("s1", "s2", "2026-08-01T09:01:20Z"),
            agent("s1", "t3", "2026-08-01T09:01:30Z"),
            typed("s1", "a2", "2026-08-01T09:02:00Z", ALSO_STANDING),
            agent("s1", "t4", "2026-08-01T09:02:10Z"),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(
        facts.recovery.after_compaction.agent_turns, 2,
        "the two turns between the boundary and the next instruction"
    );
    assert_eq!(
        facts.recovery.after_compaction.interventions, 1,
        "and the correction the operator had to make inside them"
    );
    assert_eq!(
        facts.recovery.elsewhere.agent_turns, 2,
        "the turn before the boundary and the one after the next instruction"
    );
    assert_eq!(
        facts.recovery.elsewhere.interventions, 0,
        "which nobody had to correct"
    );
}

/// The runtime writes the command wrapper; the operator never types it. So a
/// prompt that opens with the tag — pasted from a diff, or asking about this
/// very feature — is prose, and the compaction that follows it was the
/// runtime's own decision. Reading it as a command hands a later, unrelated
/// boundary an instruction nobody gave.
#[test]
fn a_prompt_the_operator_typed_is_prose_however_it_opens() {
    let pasted = "<command-name>/compact</command-name> — why does it match on this?";
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", pasted),
            agent("s1", "t1", "2026-08-01T09:00:10Z"),
            typed("s1", "a2", "2026-08-01T09:01:00Z", STANDING),
            agent("s1", "t2", "2026-08-01T09:01:10Z"),
            boundary("s1", "b1", "2026-08-01T09:02:00Z", "auto"),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(facts.compactions[0].trigger, "auto");
    assert_eq!(
        facts.compactions[0].instruction_chars, None,
        "the operator wrote the tag; they did not run the command"
    );
}

/// The runtime's own records quote the conversation back — a summary, a caveat
/// wrapper — and a quoted command sits inside that text rather than opening it.
/// The command record is the one that opens with the tag: measured over the
/// local corpus, all 373 of them do.
#[test]
fn a_runtime_record_quoting_the_command_is_not_the_command() {
    let quoted = "This session is being continued. The operator ran \
                  <command-name>/compact</command-name> with a summary request.";
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            agent("s1", "t1", "2026-08-01T09:00:10Z"),
            unclaimed("s1", "q1", "2026-08-01T09:01:00Z", quoted),
            boundary("s1", "b1", "2026-08-01T09:02:00Z", "auto"),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(
        facts.compactions[0].instruction_chars, None,
        "the record repeats the command; it is not the record that ran it"
    );
}

/// A `/compact` that produced no boundary — the operator cancelled, or the
/// session ended — belongs to no compaction. Measured over the local corpus 54
/// commands end that way, and giving one to the next boundary would report an
/// instruction against a compaction nobody gave it to.
#[test]
fn a_compact_that_produced_no_boundary_is_not_charged_to_the_next_one() {
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            compact_command("s1", "k1", "2026-08-01T09:00:30Z", "abandoned"),
            boundary("s1", "b1", "2026-08-01T09:02:00Z", "manual"),
            compact_command("s1", "k2", "2026-08-01T09:01:30Z", "the real one"),
        ],
    )]);

    let options = CollectOptions {
        with_text: true,
        ..CollectOptions::default()
    };
    let facts = session::collect(&config, &options).unwrap();

    assert_eq!(facts.compactions.len(), 1);
    assert_eq!(
        facts.compactions[0].instruction.as_deref(),
        Some("the real one"),
        "a boundary takes the latest command before it, not the oldest unclaimed"
    );
}

/// A subagent's transcript carries its parent's session id, and nothing else
/// separates the two threads. It does not carry the parent's context: a
/// compaction of the parent leaves a running subagent's own window alone, so
/// its turns ran on nothing the summary had to hold. Counting them answers a
/// rate question with another thread's denominator.
#[test]
fn a_subagents_turns_are_not_the_work_a_summary_had_to_carry() {
    let sidechain_agent = |uuid: &str, ts: &str| {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","sessionId":"s1","isSidechain":true,"message":{{"id":"m_{uuid}","content":[{{"type":"text","text":"digging"}}]}}}}"#
        )
    };
    let (_dir, config) = corpus(&[(
        "-Users-me-alpha/s1.jsonl",
        vec![
            typed("s1", "a1", "2026-08-01T09:00:00Z", STANDING),
            boundary("s1", "b1", "2026-08-01T09:01:00Z", "auto"),
            agent("s1", "t1", "2026-08-01T09:01:10Z"),
            sidechain_agent("g1", "2026-08-01T09:01:20Z"),
            // A subagent turn the runtime cut short. It is an interruption of
            // the subagent's own run, not of the work the summary carries.
            r#"{"type":"user","uuid":"x1","timestamp":"2026-08-01T09:01:25Z","sessionId":"s1","isSidechain":true,"interruptedMessageId":"m_g1","message":{"content":"stop"}}"#.to_string(),
            sidechain_agent("g2", "2026-08-01T09:01:30Z"),
            typed("s1", "a2", "2026-08-01T09:02:00Z", ALSO_STANDING),
            agent("s1", "t2", "2026-08-01T09:02:10Z"),
            sidechain_agent("g3", "2026-08-01T09:02:20Z"),
        ],
    )]);

    let facts = session::collect(&config, &CollectOptions::default()).unwrap();

    assert_eq!(
        facts.recovery.after_compaction.agent_turns, 1,
        "the one main-thread turn the summary had to carry"
    );
    assert_eq!(
        facts.recovery.elsewhere.agent_turns, 1,
        "and the one main-thread turn after the next instruction"
    );
    assert_eq!(
        facts.recovery.after_compaction.interventions, 0,
        "the subagent's own run was cut short; the operator corrected nothing here"
    );
    assert_eq!(
        facts.interventions.interventions.len(),
        1,
        "the window still records the interruption where interruptions are recorded"
    );
}
