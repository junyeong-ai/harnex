//! The repository half of a project-scoped window, against a real git tree.

use std::path::Path;
use std::process::Command;

use harness_core::session::{CommitFate, repository};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    dir
}

/// A commit that changes a file, so that reverting it has something to undo.
fn commit(dir: &Path, message: &str) -> String {
    std::fs::write(dir.join("f.txt"), message).unwrap();
    git(dir, &["add", "f.txt"]);
    git(dir, &["commit", "-q", "-m", message]);
    git(dir, &["rev-parse", "HEAD"])
}

#[test]
fn a_directory_that_is_not_a_work_tree_is_absent_rather_than_an_error() {
    let dir = TempDir::new().unwrap();
    assert!(
        repository::survey(dir.path(), &["abcdef1".into()])
            .unwrap()
            .is_none()
    );
}

#[test]
fn an_abbreviation_is_resolved_by_the_repository_and_not_by_a_prefix_match() {
    let dir = repo();
    let kept = commit(dir.path(), "kept");
    let observed = vec![kept[..9].to_string(), "0000000".to_string()];

    let facts = repository::survey(dir.path(), &observed).unwrap().unwrap();

    assert_eq!(facts.head, kept);
    assert_eq!(facts.commits[0].resolved.as_deref(), Some(kept.as_str()));
    assert_eq!(facts.commits[0].fate, CommitFate::Reachable.as_str());
    assert_eq!(facts.commits[1].fate, CommitFate::Missing.as_str());
    assert_eq!(facts.by_fate[CommitFate::Reachable.as_str()], 1);
    assert_eq!(facts.by_fate[CommitFate::Missing.as_str()], 1);
}

#[test]
fn a_commit_the_branch_no_longer_reaches_is_named_for_that_and_not_for_being_undone() {
    let dir = repo();
    commit(dir.path(), "base");
    let dropped = commit(dir.path(), "dropped");
    git(dir.path(), &["reset", "-q", "--hard", "HEAD~1"]);

    let facts = repository::survey(dir.path(), std::slice::from_ref(&dropped))
        .unwrap()
        .unwrap();

    assert_eq!(facts.commits[0].fate, CommitFate::Unreachable.as_str());
    assert!(facts.commits[0].reverted_by.is_empty());
}

#[test]
fn a_reverted_commit_names_the_commit_that_reverted_it() {
    let dir = repo();
    commit(dir.path(), "base");
    let undone = commit(dir.path(), "undone");
    git(dir.path(), &["revert", "--no-edit", &undone]);
    let reverting = git(dir.path(), &["rev-parse", "HEAD"]);

    let facts = repository::survey(dir.path(), &[undone[..9].to_string()])
        .unwrap()
        .unwrap();

    assert_eq!(facts.commits[0].fate, CommitFate::Reachable.as_str());
    assert_eq!(facts.commits[0].reverted_by, vec![reverting]);
}
