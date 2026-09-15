//! What the shipped PreToolUse arm does with an oracle it cannot use.
//!
//! A PreToolUse exit of 2 blocks the agent's action, and it is also what an
//! argument parser exits on, so the arm stands between the two: a project
//! whose oracle predates this subcommand would otherwise have every command
//! it ran refused. The arm is exercised through the template, which is what
//! adopters receive; this repository's copy is held byte-identical to it by
//! `adopted_scaffold_matches_templates`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn arm() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex/templates/common/check-floor.sh")
}

/// A stand-in oracle: `body` runs for every invocation, so a case decides
/// what `guard floor --help` and the call behind it each answer.
fn with_oracle(body: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let stub = bin.join("harnex");
    // An absolute interpreter: a case runs with a `PATH` holding only this
    // directory, which `env` would search for the shell and not find.
    std::fs::write(&stub, format!("#!/bin/sh\n{body}\n")).expect("write stub");
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let path = bin.display().to_string();
    (dir, path)
}

/// Resolved here, because a case runs the arm with a `PATH` that answers for
/// the oracle alone and would not find the shell either.
fn bash() -> PathBuf {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|d| Path::new(d).join("bash"))
        .find(|p| p.is_file())
        .expect("no bash on PATH")
}

fn run(path: &str, cwd: &Path) -> Output {
    Command::new(bash())
        .arg(arm())
        .current_dir(cwd)
        .env("PATH", path)
        .output()
        .expect("run the arm")
}

#[test]
fn an_absent_oracle_leaves_the_tool_call_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = run("", dir.path());
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "and says nothing about it");
}

#[test]
fn an_oracle_without_this_subcommand_leaves_the_tool_call_alone() {
    // What clap exits on an unrecognised subcommand is 2 — the block code.
    let (dir, path) = with_oracle("echo \"error: unrecognized subcommand\" >&2; exit 2");
    let output = run(&path, dir.path());
    assert_eq!(
        output.status.code(),
        Some(0),
        "a parser's usage error is not a floor violation"
    );
}

#[test]
fn a_block_from_the_oracle_reaches_the_runtime_intact() {
    let (dir, path) = with_oracle(
        "if [ \"${3:-}\" = --help ]; then exit 0; fi\n\
         echo '✗ floor-integrity: the reason' >&2\nexit 2",
    );
    let output = run(&path, dir.path());
    assert_eq!(output.status.code(), Some(2), "the block passes through");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("floor-integrity"),
        "with its reason"
    );
}

#[test]
fn an_allowance_from_the_oracle_passes_through_silently() {
    let (dir, path) = with_oracle("exit 0");
    let output = run(&path, dir.path());
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}
