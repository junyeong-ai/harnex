//! Integration tests for the codegen module.

use harness_core::codegen::SentinelSyncer;
use harness_core::config::{CodegenConfig, CodegenGroupDecl, SentinelTargetDecl};
use std::path::PathBuf;
use tempfile::TempDir;

fn setup(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let source = tmp.path().join("src.toml");
    std::fs::write(
        &source,
        r#"
[kinds]
allowed = ["rule", "skill", "hook"]
"#,
    )
    .unwrap();
    let target = tmp.path().join("nodex.toml");
    std::fs::write(
        &target,
        r#"# top
# BEGIN kinds-allowed
allowed = ["old"]
# END kinds-allowed
# bottom
"#,
    )
    .unwrap();
    (source, target)
}

#[test]
fn sync_applies_changes_atomically() {
    let tmp = TempDir::new().unwrap();
    let (source, target) = setup(&tmp);
    let cfg = CodegenConfig {
        groups: vec![CodegenGroupDecl {
            name: "kinds".into(),
            source: source.strip_prefix(tmp.path()).unwrap().to_path_buf(),
            source_key: "kinds.allowed".into(),
            targets: vec![SentinelTargetDecl {
                path: target.strip_prefix(tmp.path()).unwrap().to_path_buf(),
                begin: "# BEGIN kinds-allowed".into(),
                end: "# END kinds-allowed".into(),
                format: "toml-array-assignment".into(),
                name: Some("allowed".into()),
            }],
        }],
    };
    let outcomes = SentinelSyncer::new(&cfg, tmp.path()).sync().unwrap();
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].changed);
    let new_content = std::fs::read_to_string(&target).unwrap();
    assert!(new_content.contains("allowed = [\"rule\", \"skill\", \"hook\"]"));
    assert!(new_content.contains("# top"));
    assert!(new_content.contains("# bottom"));
}

#[test]
fn check_reports_drift_without_writing() {
    let tmp = TempDir::new().unwrap();
    let (source, target) = setup(&tmp);
    let original = std::fs::read_to_string(&target).unwrap();
    let cfg = CodegenConfig {
        groups: vec![CodegenGroupDecl {
            name: "kinds".into(),
            source: source.strip_prefix(tmp.path()).unwrap().to_path_buf(),
            source_key: "kinds.allowed".into(),
            targets: vec![SentinelTargetDecl {
                path: target.strip_prefix(tmp.path()).unwrap().to_path_buf(),
                begin: "# BEGIN kinds-allowed".into(),
                end: "# END kinds-allowed".into(),
                format: "toml-array-assignment".into(),
                name: Some("allowed".into()),
            }],
        }],
    };
    let outcomes = SentinelSyncer::new(&cfg, tmp.path()).check().unwrap();
    assert!(outcomes[0].changed);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), original);
}

#[test]
fn idempotent_no_change_on_second_sync() {
    let tmp = TempDir::new().unwrap();
    let (source, target) = setup(&tmp);
    let cfg = CodegenConfig {
        groups: vec![CodegenGroupDecl {
            name: "kinds".into(),
            source: source.strip_prefix(tmp.path()).unwrap().to_path_buf(),
            source_key: "kinds.allowed".into(),
            targets: vec![SentinelTargetDecl {
                path: target.strip_prefix(tmp.path()).unwrap().to_path_buf(),
                begin: "# BEGIN kinds-allowed".into(),
                end: "# END kinds-allowed".into(),
                format: "toml-array-assignment".into(),
                name: Some("allowed".into()),
            }],
        }],
    };
    SentinelSyncer::new(&cfg, tmp.path()).sync().unwrap();
    let outcomes = SentinelSyncer::new(&cfg, tmp.path()).sync().unwrap();
    assert!(!outcomes[0].changed);
}

#[test]
fn missing_sentinel_errors() {
    let tmp = TempDir::new().unwrap();
    let (source, target) = setup(&tmp);
    let cfg = CodegenConfig {
        groups: vec![CodegenGroupDecl {
            name: "kinds".into(),
            source: source.strip_prefix(tmp.path()).unwrap().to_path_buf(),
            source_key: "kinds.allowed".into(),
            targets: vec![SentinelTargetDecl {
                path: target.strip_prefix(tmp.path()).unwrap().to_path_buf(),
                begin: "# BEGIN does-not-exist".into(),
                end: "# END does-not-exist".into(),
                format: "toml-array-assignment".into(),
                name: Some("allowed".into()),
            }],
        }],
    };
    let err = SentinelSyncer::new(&cfg, tmp.path()).sync().unwrap_err();
    assert_eq!(
        err.code(),
        harness_core::error::ErrorCode::CodegenSentinelMissing
    );
}

#[test]
fn missing_source_key_errors() {
    let tmp = TempDir::new().unwrap();
    let (source, target) = setup(&tmp);
    let cfg = CodegenConfig {
        groups: vec![CodegenGroupDecl {
            name: "kinds".into(),
            source: source.strip_prefix(tmp.path()).unwrap().to_path_buf(),
            source_key: "kinds.does_not_exist".into(),
            targets: vec![SentinelTargetDecl {
                path: target.strip_prefix(tmp.path()).unwrap().to_path_buf(),
                begin: "# BEGIN kinds-allowed".into(),
                end: "# END kinds-allowed".into(),
                format: "toml-array-assignment".into(),
                name: Some("allowed".into()),
            }],
        }],
    };
    let err = SentinelSyncer::new(&cfg, tmp.path()).sync().unwrap_err();
    assert_eq!(
        err.code(),
        harness_core::error::ErrorCode::CodegenSourceKeyMissing
    );
}
