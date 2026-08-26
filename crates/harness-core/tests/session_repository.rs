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
        repository::survey(dir.path(), &["abcdef1".into()], None)
            .unwrap()
            .is_none()
    );
}

#[test]
fn an_abbreviation_is_resolved_by_the_repository_and_not_by_a_prefix_match() {
    let dir = repo();
    let kept = commit(dir.path(), "kept");
    let observed = vec![kept[..9].to_string(), "0000000".to_string()];

    let facts = repository::survey(dir.path(), &observed, None)
        .unwrap()
        .unwrap();

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

    let facts = repository::survey(dir.path(), std::slice::from_ref(&dropped), None)
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

    let facts = repository::survey(dir.path(), &[undone[..9].to_string()], None)
        .unwrap()
        .unwrap();

    assert_eq!(facts.commits[0].fate, CommitFate::Reachable.as_str());
    assert_eq!(facts.commits[0].reverted_by, vec![reverting]);
}

#[test]
fn a_window_with_thousands_of_commits_returns_rather_than_blocking() {
    let dir = repo();
    let head = commit(dir.path(), "one");
    // Enough queries that a piped stdin would fill the kernel buffer before
    // anything drained stdout, which is where this used to stop answering.
    let observed: Vec<String> = (0..10_000).map(|_| head[..9].to_string()).collect();

    let path = dir.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(repository::survey(&path, &observed, None));
    });

    // A hang is the failure this guards against, so it is reported as one
    // rather than left to stall the run.
    let facts = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("survey returned")
        .expect("survey succeeded")
        .expect("the tempdir is a work tree");
    assert_eq!(facts.commits.len(), 10_000);
    assert_eq!(facts.by_fate[CommitFate::Reachable.as_str()], 10_000);
}

#[test]
fn the_observed_commits_are_reported_beside_what_the_repository_counts() {
    let dir = repo();
    let first = commit(dir.path(), "one");
    commit(dir.path(), "two");
    commit(dir.path(), "three");
    let from: jiff::Timestamp = "2000-01-01T00:00:00Z".parse().unwrap();
    let to = jiff::Timestamp::now();

    // One of three observed: the transcript records a commit only sometimes,
    // and a consumer must be able to see by how much.
    let facts = repository::survey(dir.path(), &[first[..9].to_string()], Some((from, to)))
        .unwrap()
        .unwrap();

    assert_eq!(facts.commits.len(), 1);
    assert_eq!(facts.commits_in_span, Some(3));
}

#[test]
fn a_revision_expression_the_transcript_invented_is_not_asked_of_git() {
    let dir = repo();
    let head = commit(dir.path(), "one");
    commit(dir.path(), "two");

    let facts = repository::survey(
        dir.path(),
        &["HEAD~1".into(), "main".into(), head[..9].to_string()],
        None,
    )
    .unwrap()
    .unwrap();

    assert_eq!(facts.commits[0].fate, CommitFate::Missing.as_str());
    assert_eq!(facts.commits[1].fate, CommitFate::Missing.as_str());
    assert_eq!(
        facts.commits[2].fate,
        CommitFate::Reachable.as_str(),
        "the one that is an abbreviation still resolves"
    );
}

#[test]
fn the_revert_trailer_is_a_line_and_not_a_phrase_in_a_paragraph() {
    let dir = repo();
    let undone = commit(dir.path(), "undone");
    std::fs::write(dir.path().join("f.txt"), "talk").unwrap();
    git(dir.path(), &["add", "f.txt"]);
    git(
        dir.path(),
        &[
            "commit",
            "-q",
            "-m",
            &format!("discuss\n\nsomeone said This reverts commit {undone} in review"),
        ],
    );

    let facts = repository::survey(dir.path(), std::slice::from_ref(&undone), None)
        .unwrap()
        .unwrap();

    assert!(
        facts.commits[0].reverted_by.is_empty(),
        "a sentence about a revert is not a revert"
    );
}
