//! The session-start probe, run rather than read.
//!
//! `scaffold_git_hooks_match_the_probe` holds the hook names the probe
//! declares to the manifest, and a declaration is not a use: the loop reading
//! them can stop reading them while that guard stays green. What the probe
//! reports also reaches every session's context, so a wrong report is worse
//! than a missing one — these cases build a repository in each state and
//! assert on what an operator would actually see.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

fn template() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex/templates/common/session-start.sh")
}

/// A repository holding the shipped probe beside the named git hooks, each
/// executable unless a case says otherwise.
fn clone_with(hooks: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let hooks_dir = dir.path().join("hooks");
    std::fs::create_dir_all(&hooks_dir).expect("hooks dir");
    std::fs::copy(template(), hooks_dir.join("session-start.sh")).expect("copy probe");
    for hook in hooks {
        let path = hooks_dir.join(hook);
        std::fs::write(&path, "#!/bin/sh\nexit 1\n").expect("write hook");
        set_mode(&path, 0o755);
    }
    let status = common::git(dir.path())
        .args(["init", "-q", "."])
        .status()
        .expect("git init runs");
    assert!(status.success(), "git init failed");
    dir
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

fn config(root: &Path, args: &[&str]) {
    let status = common::git(root)
        .arg("config")
        .args(args)
        .status()
        .expect("git config runs");
    assert!(status.success(), "git config {args:?} failed");
}

/// What a session would be given, standing in the repository root.
fn probe(root: &Path) -> String {
    let output = Command::new("bash")
        .arg(root.join("hooks/session-start.sh"))
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("the probe runs");
    assert!(
        output.status.success(),
        "the probe must never fail a session"
    );
    String::from_utf8(output.stdout).expect("probe output is UTF-8")
}

#[test]
fn a_clone_git_will_not_run_the_hooks_of_is_told_so() {
    let dir = clone_with(&["pre-commit"]);
    let said = probe(dir.path());
    assert!(
        said.contains("are not armed"),
        "an unarmed clone hears about it: {said}"
    );
}

#[test]
fn the_report_does_not_rest_on_one_of_the_two_hooks() {
    // The manifest ships two, and a project that kept only the second holds a
    // gate that would never run. A probe reading its loop variable reports
    // here; one that asks after `pre-commit` whatever it was handed does not.
    let dir = clone_with(&["commit-msg"]);
    let said = probe(dir.path());
    assert!(
        said.contains("are not armed"),
        "a clone shipping only commit-msg hears about it too: {said}"
    );
}

#[test]
fn an_armed_clone_hears_nothing() {
    let dir = clone_with(&["pre-commit", "commit-msg"]);
    config(dir.path(), &["core.hooksPath", "hooks"]);
    let said = probe(dir.path());
    assert!(
        !said.contains("are not armed") && !said.contains("executable bit"),
        "nothing is wrong, so nothing is said: {said}"
    );
    assert!(
        said.starts_with("Branch:"),
        "the ordinary facts still print: {said}"
    );
}

#[test]
fn a_hook_git_would_skip_is_reported_where_the_clone_is_armed() {
    let dir = clone_with(&["pre-commit"]);
    config(dir.path(), &["core.hooksPath", "hooks"]);
    set_mode(&dir.path().join("hooks/pre-commit"), 0o644);
    let said = probe(dir.path());
    assert!(
        said.contains("executable bit"),
        "git skips a hook without the bit, and says so only on its own stderr: {said}"
    );
    assert!(
        !said.contains("are not armed"),
        "git does run hooks from here, so saying otherwise would name the wrong repair: {said}"
    );
}

#[test]
fn the_command_it_prints_arms_the_clone_it_was_printed_in() {
    // The one property that makes the report worth printing. A local
    // `core.hooksPath` is the shape every hook manager leaves behind, and it
    // is where naming the wrong scope would go unnoticed.
    for existing in [None, Some(".husky")] {
        let dir = clone_with(&["pre-commit"]);
        if let Some(value) = existing {
            config(dir.path(), &["core.hooksPath", value]);
        }
        let said = probe(dir.path());
        let command = said
            .lines()
            .find_map(|line| line.split('`').nth(1))
            .unwrap_or_else(|| panic!("the report names a command: {said}"));
        // The report names the whole command; `config` runs it as git already.
        let args: Vec<&str> = command.split_whitespace().skip(2).collect();
        config(dir.path(), &args);
        assert!(
            !probe(dir.path()).contains("are not armed"),
            "`{command}` was printed as the repair and left the clone unarmed"
        );
    }
}

#[test]
fn the_scope_it_names_is_the_scope_that_holds_the_setting() {
    // `--worktree` is `--local` wearing another name until `worktreeConfig` is
    // enabled, so a probe asking it alone reads any ordinary local value — the
    // shape every hook manager leaves behind — as worktree-scoped. Both write
    // the same file today, which is why the wrong word costs nothing until the
    // extension is on and the advice then arms one worktree and no other. That
    // makes the word, not the command's effect, the thing to assert.
    let local = clone_with(&["pre-commit"]);
    config(local.path(), &["core.hooksPath", ".husky"]);
    assert!(
        !probe(local.path()).contains("--worktree"),
        "a local value under a clone with no worktree config is not worktree-scoped"
    );

    let scoped = clone_with(&["pre-commit"]);
    config(scoped.path(), &["extensions.worktreeConfig", "true"]);
    config(
        scoped.path(),
        &["--worktree", "core.hooksPath", "elsewhere"],
    );
    assert!(
        probe(scoped.path()).contains("--worktree"),
        "a worktree-scoped value shadows anything written to the shared scope"
    );
}
