//! What the shipped pre-commit does with the arms in `pre-commit.d/`.
//!
//! A commit gate an agent meets reports one failure per attempt if the hook
//! stops at the first, and the agent pays a commit round-trip for each. So
//! every arm runs and the commit is blocked if any failed — except on a
//! signal, which is someone stopping the commit. The hook is exercised through
//! the template under every bash on the machine; this repository's copy is
//! held byte-identical to it by `adopted_scaffold_matches_templates`.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

mod common;

/// A hook directory holding the template and one arm per `(name, status)`.
/// Each arm leaves a mark named after itself before it exits.
fn hooks(arms: &[(&str, u8)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex/templates/common/git-hooks/pre-commit");
    std::fs::copy(template, dir.path().join("pre-commit")).expect("copy the hook");
    let armed = dir.path().join("pre-commit.d");
    std::fs::create_dir(&armed).expect("pre-commit.d");
    for (name, status) in arms {
        let arm = armed.join(name);
        std::fs::write(
            &arm,
            format!("#!/bin/sh\ntouch \"$(dirname \"$0\")/../ran-{name}\"\nexit {status}\n"),
        )
        .expect("write arm");
        std::fs::set_permissions(&arm, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    dir
}

fn commit(bash: &str, dir: &Path) -> Output {
    Command::new(bash)
        .arg(dir.join("pre-commit"))
        .env("HARNEX_SKIP_GITLEAKS", "1")
        .output()
        .expect("run the hook")
}

fn ran(dir: &Path, name: &str) -> bool {
    dir.join(format!("ran-{name}")).exists()
}

#[test]
fn every_arm_runs_and_any_failure_blocks_the_commit() {
    for bash in common::bashes() {
        let dir = hooks(&[("10-lint", 1), ("20-test", 0), ("30-check", 2)]);
        let out = commit(bash, dir.path());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{bash}: {stderr}");
        for name in ["10-lint", "20-test", "30-check"] {
            assert!(ran(dir.path(), name), "{bash}: {name} never ran");
        }
        assert!(
            stderr.contains("10-lint (exit 1)") && stderr.contains("30-check (exit 2)"),
            "{bash}: the block names each failed arm: {stderr}"
        );
    }
}

#[test]
fn an_interrupted_arm_stops_the_commit_there() {
    for bash in common::bashes() {
        let dir = hooks(&[("10-lint", 130), ("20-test", 0)]);
        let out = commit(bash, dir.path());
        assert_eq!(out.status.code(), Some(130), "{bash}");
        assert!(
            !ran(dir.path(), "20-test"),
            "{bash}: an arm ran after the interrupt"
        );
    }
}

#[test]
fn passing_arms_let_the_commit_through() {
    for bash in common::bashes() {
        let dir = hooks(&[("10-lint", 0), ("20-test", 0)]);
        let out = commit(bash, dir.path());
        assert_eq!(
            out.status.code(),
            Some(0),
            "{bash}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
