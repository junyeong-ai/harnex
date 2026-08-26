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

    assert_eq!(facts.prompts.authored_prompts, 4);
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

    assert_eq!(facts.prompts.authored_prompts, 1);
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

    assert_eq!(facts.prompts.authored_prompts, 1);
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
