//! The repository half of a project-scoped window, against a real git tree.

use std::path::{Path, PathBuf};
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
    // The identity the commits carry is also the one the repository names, as
    // in any real checkout: the span count asks git which commits are this
    // author's, and a repository whose config disagrees with its own history
    // is a fixture shape rather than a project.
    git(dir.path(), &["config", "user.name", "t"]);
    git(dir.path(), &["config", "user.email", "t@t"]);
    dir
}

/// A commit that changes a file, so that reverting it has something to undo.
fn commit(dir: &Path, message: &str) -> String {
    std::fs::write(dir.join("f.txt"), message).unwrap();
    git(dir, &["add", "f.txt"]);
    git(dir, &["commit", "-q", "-m", message]);
    git(dir, &["rev-parse", "HEAD"])
}

/// A commit that changes several files at once, none of them the one `commit`
/// writes, so a path list cannot be confused for the fixture's own file.
fn commit_touching(dir: &Path, message: &str, paths: &[&str]) -> String {
    for path in paths {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, message).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
    git(dir, &["rev-parse", "HEAD"])
}

/// Where git says the work tree is, which is what a reported path is joined to.
fn work_tree(dir: &Path) -> PathBuf {
    PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"]))
}

#[test]
fn a_commit_reports_the_paths_it_changed_including_the_first_one_in_the_tree() {
    let dir = repo();
    let root = commit_touching(dir.path(), "root", &["src/lib.rs", "README.md"]);
    let later = commit_touching(dir.path(), "later", &["src/lib.rs", "tests/it.rs"]);

    let touched = repository::paths_touched(dir.path(), &[root.clone(), later.clone()]).unwrap();

    let tree = work_tree(dir.path());
    assert_eq!(
        touched[&root],
        vec![tree.join("README.md"), tree.join("src/lib.rs")],
        "a root commit has no parent and still changed every file in it"
    );
    assert_eq!(
        touched[&later],
        vec![tree.join("src/lib.rs"), tree.join("tests/it.rs")]
    );
}

#[test]
fn a_file_named_like_an_object_id_is_a_path_and_not_a_commit() {
    let dir = repo();
    let hex = "0123456789abcdef0123456789abcdef01234567";
    let sha = commit_touching(dir.path(), "hashed", &[hex, "src/lib.rs"]);

    let touched = repository::paths_touched(dir.path(), std::slice::from_ref(&sha)).unwrap();

    assert_eq!(touched.len(), 1, "one commit was asked about");
    let tree = work_tree(dir.path());
    assert_eq!(
        touched[&sha],
        vec![tree.join(hex), tree.join("src/lib.rs")],
        "a name shaped like an object id is still a name"
    );
}

#[test]
fn a_merge_changed_nothing_of_its_own_and_reports_no_paths() {
    let dir = repo();
    let base = commit_touching(dir.path(), "base", &["src/lib.rs"]);
    git(dir.path(), &["checkout", "-q", "-b", "side", &base]);
    commit_touching(dir.path(), "side", &["side.rs"]);
    git(dir.path(), &["checkout", "-q", "main"]);
    commit_touching(dir.path(), "main", &["main.rs"]);
    git(
        dir.path(),
        &["merge", "-q", "--no-ff", "side", "-m", "merge"],
    );
    let merge = git(dir.path(), &["rev-parse", "HEAD"]);

    let touched = repository::paths_touched(dir.path(), std::slice::from_ref(&merge)).unwrap();

    assert_eq!(
        touched[&merge],
        Vec::<std::path::PathBuf>::new(),
        "present and empty, which is what a merge changed on its own"
    );
}

#[test]
fn a_revision_expression_is_not_asked_of_git_here_either() {
    let dir = repo();
    let sha = commit_touching(dir.path(), "one", &["src/lib.rs"]);

    let touched =
        repository::paths_touched(dir.path(), &["HEAD~1".into(), "main".into(), sha.clone()])
            .unwrap();

    assert_eq!(touched.keys().collect::<Vec<_>>(), vec![&sha]);
}

#[test]
fn a_path_outside_ascii_is_reported_as_it_is_written() {
    let dir = repo();
    let sha = commit_touching(dir.path(), "korean", &["문서/설계.md"]);

    let touched = repository::paths_touched(dir.path(), std::slice::from_ref(&sha)).unwrap();

    assert_eq!(
        touched[&sha],
        vec![work_tree(dir.path()).join("문서/설계.md")]
    );
}

#[test]
fn no_commits_asks_git_nothing() {
    let dir = repo();
    assert!(
        repository::paths_touched(dir.path(), &[])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_harness_is_the_last_commit_to_touch_it_and_whether_it_has_moved_since() {
    let dir = repo();
    commit_touching(dir.path(), "code", &["src/lib.rs"]);
    let rules = commit_touching(dir.path(), "rules", &[".claude/rules/a.md"]);
    let later = commit_touching(dir.path(), "more code", &["src/main.rs"]);
    let paths = vec![".claude".to_string(), "CLAUDE.md".to_string()];

    let state = repository::harness_state(dir.path(), &paths)
        .unwrap()
        .unwrap();

    assert_eq!(
        state.head.as_deref(),
        Some(rules.as_str()),
        "a commit that changed no harness path did not move the harness"
    );
    assert_ne!(state.head.as_deref(), Some(later.as_str()));
    assert!(!state.uncommitted);

    std::fs::write(dir.path().join(".claude/rules/a.md"), "edited").unwrap();
    let dirty = repository::harness_state(dir.path(), &paths)
        .unwrap()
        .unwrap();

    assert_eq!(dirty.head, state.head);
    assert!(
        dirty.uncommitted,
        "the harness on disk is no longer the one that commit names"
    );
}

#[test]
fn a_project_whose_harness_was_never_committed_names_no_commit() {
    let dir = repo();
    commit_touching(dir.path(), "code", &["src/lib.rs"]);

    let state = repository::harness_state(dir.path(), &[".claude".to_string()])
        .unwrap()
        .unwrap();

    assert_eq!(state.head, None);
    assert!(!state.uncommitted);
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
    assert_eq!(facts.authored_in_span, Some(3));
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

#[test]
fn a_project_below_the_work_tree_still_reports_a_path_that_can_be_opened() {
    let dir = repo();
    let sha = commit_touching(dir.path(), "nested", &["crates/core/src/lib.rs"]);
    let project = dir.path().join("crates/core");

    let touched = repository::paths_touched(&project, std::slice::from_ref(&sha)).unwrap();

    let reported = &touched[&sha][0];
    assert!(
        reported.is_file(),
        "git spells a path from the work tree root, not from the project it was \
         asked in, so joining it to the project would name a file that is not there: {}",
        reported.display()
    );
    assert_eq!(
        *reported,
        work_tree(&project).join("crates/core/src/lib.rs")
    );
}

#[test]
fn project_memory_beside_the_code_it_governs_is_part_of_the_harness() {
    let dir = repo();
    commit_touching(dir.path(), "root memory", &["CLAUDE.md"]);
    let nested = commit_touching(dir.path(), "crate memory", &["crates/core/CLAUDE.md"]);

    let state =
        repository::harness_state(dir.path(), &harness_core::config::default_harness_paths())
            .unwrap()
            .expect("a work tree answers");

    assert_eq!(
        state.head.as_deref(),
        Some(nested.as_str()),
        "a bare pathspec is anchored at the work tree root, so memory nested \
         beside a crate moves the harness and only `**/` sees it"
    );
    assert!(!state.uncommitted);
}

#[test]
fn a_window_scoped_below_the_root_is_told_about_the_whole_harness() {
    let dir = repo();
    let root_memory = commit_touching(dir.path(), "root memory", &["CLAUDE.md"]);
    commit_touching(dir.path(), "unrelated", &["crates/core/src/lib.rs"]);
    let project = dir.path().join("crates/core");

    let state = repository::harness_state(&project, &harness_core::config::default_harness_paths())
        .unwrap()
        .expect("a directory inside a work tree is one");

    assert_eq!(
        state.head.as_deref(),
        Some(root_memory.as_str()),
        "git resolves a pathspec against the directory it runs in, so a window \
         scoped to a package would answer about that package's harness alone \
         and call a root memory change no change at all"
    );
}

#[test]
fn a_repository_with_nothing_committed_yet_answers_rather_than_failing() {
    let dir = repo();

    assert!(
        repository::survey(dir.path(), &[], None).unwrap().is_none(),
        "an unborn HEAD is a repository with nothing in it, not a git failure"
    );
    assert!(
        repository::harness_state(dir.path(), &harness_core::config::default_harness_paths())
            .unwrap()
            .is_none(),
        "and the harness it does not have yet is absent, not unreadable"
    );
}

#[test]
fn the_span_counts_this_author_and_not_the_repository() {
    let dir = repo();
    commit(dir.path(), "mine");
    let out = std::process::Command::new("git")
        .args(["commit", "-q", "--allow-empty", "-m", "theirs"])
        .current_dir(dir.path())
        .env("GIT_AUTHOR_NAME", "other")
        .env("GIT_AUTHOR_EMAIL", "other@elsewhere")
        .env("GIT_COMMITTER_NAME", "other")
        .env("GIT_COMMITTER_EMAIL", "other@elsewhere")
        .output()
        .expect("git runs");
    assert!(out.status.success());

    let made_at: jiff::Timestamp = git(dir.path(), &["log", "-1", "--format=%aI"])
        .parse()
        .unwrap();
    let span = Some((
        made_at - jiff::SignedDuration::from_hours(1),
        made_at + jiff::SignedDuration::from_hours(1),
    ));
    let facts = repository::survey(dir.path(), &[], span).unwrap().unwrap();

    assert_eq!(
        facts.authored_in_span,
        Some(1),
        "a teammate's commit is the repository's output, not a floor this \
         operator's recorded commits should be read against"
    );
}
