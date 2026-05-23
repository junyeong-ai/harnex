//! Integration tests for the telemetry module.

use harness_core::config::{TelemetryConfig, TelemetryKindDecl};
use harness_core::error::Error;
use harness_core::telemetry::{JsonlStorage, TelemetryAppender, TelemetryQuery};
use jiff::ToSpan;
use tempfile::TempDir;

fn config_with_skill_invoked(dir: std::path::PathBuf) -> TelemetryConfig {
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
                    "outcome": {"type": "string", "enum": ["ok", "warn", "fail"]},
                    "duration_ms": {"type": "integer"}
                }
            }),
        }],
    }
}

#[test]
fn append_and_count_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let cfg = config_with_skill_invoked(tmp.path().to_path_buf());

    {
        let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
        let mut appender = TelemetryAppender::new(&cfg, storage).unwrap();
        appender
            .append(
                "skill-invoked",
                serde_json::json!({"skill": "a", "outcome": "ok"}),
            )
            .unwrap();
        appender
            .append(
                "skill-invoked",
                serde_json::json!({"skill": "b", "outcome": "warn", "duration_ms": 42}),
            )
            .unwrap();
    }

    let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
    let query = TelemetryQuery::new(storage);
    assert_eq!(query.count("skill-invoked", None).unwrap(), 2);
}

#[test]
fn rejects_unknown_kind() {
    let tmp = TempDir::new().unwrap();
    let cfg = config_with_skill_invoked(tmp.path().to_path_buf());
    let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
    let mut appender = TelemetryAppender::new(&cfg, storage).unwrap();
    let err = appender
        .append("does-not-exist", serde_json::json!({}))
        .unwrap_err();
    assert!(matches!(err, Error::TelemetryKindUnknown { .. }));
}

#[test]
fn rejects_missing_required_field() {
    let tmp = TempDir::new().unwrap();
    let cfg = config_with_skill_invoked(tmp.path().to_path_buf());
    let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
    let mut appender = TelemetryAppender::new(&cfg, storage).unwrap();
    let err = appender
        .append("skill-invoked", serde_json::json!({"skill": "a"}))
        .unwrap_err();
    assert!(matches!(err, Error::TelemetryPayloadInvalid { .. }));
    assert!(format!("{err}").contains("outcome"));
}

#[test]
fn rejects_undeclared_field() {
    let tmp = TempDir::new().unwrap();
    let cfg = config_with_skill_invoked(tmp.path().to_path_buf());
    let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
    let mut appender = TelemetryAppender::new(&cfg, storage).unwrap();
    let err = appender
        .append(
            "skill-invoked",
            serde_json::json!({"skill": "a", "outcome": "ok", "rogue": "extra"}),
        )
        .unwrap_err();
    assert!(matches!(err, Error::TelemetryPayloadInvalid { .. }));
    assert!(format!("{err}").contains("rogue"));
}

#[test]
fn rejects_type_mismatch() {
    let tmp = TempDir::new().unwrap();
    let cfg = config_with_skill_invoked(tmp.path().to_path_buf());
    let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
    let mut appender = TelemetryAppender::new(&cfg, storage).unwrap();
    let err = appender
        .append(
            "skill-invoked",
            serde_json::json!({"skill": "a", "outcome": "ok", "duration_ms": "not-integer"}),
        )
        .unwrap_err();
    assert!(matches!(err, Error::TelemetryPayloadInvalid { .. }));
}

#[test]
fn rejects_enum_violation() {
    let tmp = TempDir::new().unwrap();
    let cfg = config_with_skill_invoked(tmp.path().to_path_buf());
    let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
    let mut appender = TelemetryAppender::new(&cfg, storage).unwrap();
    let err = appender
        .append(
            "skill-invoked",
            serde_json::json!({"skill": "a", "outcome": "bogus"}),
        )
        .unwrap_err();
    assert!(matches!(err, Error::TelemetryPayloadInvalid { .. }));
}

#[test]
fn report_aggregates_per_kind_and_window() {
    let tmp = TempDir::new().unwrap();
    let cfg = config_with_skill_invoked(tmp.path().to_path_buf());
    {
        let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
        let mut appender = TelemetryAppender::new(&cfg, storage).unwrap();
        for _ in 0..5 {
            appender
                .append(
                    "skill-invoked",
                    serde_json::json!({"skill": "a", "outcome": "ok"}),
                )
                .unwrap();
        }
    }
    let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
    let query = TelemetryQuery::new(storage);
    let summary = query.report(&[1, 7, 30, 90], None).unwrap();
    assert_eq!(summary.kinds.len(), 1);
    let k = &summary.kinds[0];
    assert_eq!(k.kind, "skill-invoked");
    assert_eq!(k.total, 5);
    assert!(k.first_seen.is_some());
    assert!(k.last_seen.is_some());
    assert_eq!(k.last_n_days.get(&1), Some(&5));
    assert_eq!(k.last_n_days.get(&90), Some(&5));
    assert_eq!(summary.windows, vec![1, 7, 30, 90]);
}

#[test]
fn report_kind_filter_restricts_output() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = config_with_skill_invoked(tmp.path().to_path_buf());
    cfg.kinds.push(TelemetryKindDecl {
        name: "hook-fired".into(),
        payload_schema: serde_json::json!({
            "type": "object",
            "required": ["event"],
            "properties": {"event": {"type": "string"}}
        }),
    });
    {
        let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
        let mut appender = TelemetryAppender::new(&cfg, storage).unwrap();
        appender
            .append(
                "skill-invoked",
                serde_json::json!({"skill": "x", "outcome": "ok"}),
            )
            .unwrap();
        appender
            .append("hook-fired", serde_json::json!({"event": "Stop"}))
            .unwrap();
    }
    let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
    let query = TelemetryQuery::new(storage);
    let summary = query.report(&[7], Some("hook-fired")).unwrap();
    assert_eq!(summary.kinds.len(), 1);
    assert_eq!(summary.kinds[0].kind, "hook-fired");
    assert_eq!(summary.kinds[0].total, 1);
}

#[test]
fn report_empty_ledger_returns_no_kinds() {
    let tmp = TempDir::new().unwrap();
    let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
    let query = TelemetryQuery::new(storage);
    let summary = query.report(&[7, 30], None).unwrap();
    assert!(summary.kinds.is_empty());
    assert_eq!(summary.windows, vec![7, 30]);
}

#[test]
fn count_with_since_filter() {
    let tmp = TempDir::new().unwrap();
    let cfg = config_with_skill_invoked(tmp.path().to_path_buf());
    let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
    let mut appender = TelemetryAppender::new(&cfg, storage).unwrap();
    appender
        .append(
            "skill-invoked",
            serde_json::json!({"skill": "a", "outcome": "ok"}),
        )
        .unwrap();

    let storage = JsonlStorage::new(tmp.path().to_path_buf(), 10);
    let query = TelemetryQuery::new(storage);
    let now = jiff::Timestamp::now();
    // Past timestamp — should include
    let past = now - 1.hours();
    assert_eq!(query.count("skill-invoked", Some(past)).unwrap(), 1);
    // Future timestamp — should exclude
    let future = now + 1.hours();
    assert_eq!(query.count("skill-invoked", Some(future)).unwrap(), 0);
}
