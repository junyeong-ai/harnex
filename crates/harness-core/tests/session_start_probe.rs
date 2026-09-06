//! The session-start probe, run rather than read.
//!
//! `scaffold_git_hooks_match_the_probe` holds the hook names the probe
//! declares to the manifest, and a declaration is not a use: the loop reading
//! them can stop reading them while that guard stays green. What the probe
//! reports also reaches every session's context, so a wrong report is worse
//! than a missing one — these cases build a repository in each state and
//! assert on what an operator would actually see.
//!
//! Where a report names a command, the command is run the way its reader runs
//! it: through a shell, from where the session stands. Handing the words to
//! `Command` as separate arguments would spell a repair no quoting defect can
//! reach, and grade the probe against a harness that cannot fail.

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
    populate(dir.path(), &dir.path().join("hooks"), hooks);
    dir
}

/// Initialises `root` as a repository and fills `hooks_dir` with the probe and
/// the named hooks.
fn populate(root: &Path, hooks_dir: &Path, hooks: &[&str]) {
    std::fs::create_dir_all(hooks_dir).expect("hooks dir");
    std::fs::copy(template(), hooks_dir.join("session-start.sh")).expect("copy probe");
    for hook in hooks {
        let path = hooks_dir.join(hook);
        std::fs::write(&path, "#!/bin/sh\nexit 1\n").expect("write hook");
        set_mode(&path, 0o755);
    }
    let status = common::git(root)
        .args(["init", "-q", "."])
        .status()
        .expect("git init runs");
    assert!(status.success(), "git init failed");
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).expect("stat").permissions().mode() & 0o777
}

fn config(root: &Path, args: &[&str]) {
    let status = common::git(root)
        .arg("config")
        .args(args)
        .status()
        .expect("git config runs");
    assert!(status.success(), "git config {args:?} failed");
}

/// What a session would be given, standing in `root`, from a probe at `script`.
fn probe_from(root: &Path, script: &Path) -> String {
    probe_with(root, script, &[])
}

/// The common case: the probe sits in `root/hooks`.
fn probe(root: &Path) -> String {
    probe_from(root, &root.join("hooks/session-start.sh"))
}

/// As [`probe_from`], with `env` overriding the child's environment.
fn probe_with(root: &Path, script: &Path, env: &[(&str, &str)]) -> String {
    let mut command = Command::new("bash");
    command
        .arg(script)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().expect("the probe runs");
    assert!(
        output.status.success(),
        "the probe must never fail a session"
    );
    String::from_utf8(output.stdout).expect("probe output is UTF-8")
}

/// The command a report names, between its backticks.
fn printed_command(said: &str) -> &str {
    said.lines()
        .find_map(|line| line.split('`').nth(1))
        .unwrap_or_else(|| panic!("the report names a command: {said}"))
}

/// Runs that command through a shell, from `cwd`, and returns it.
fn run_printed<'a>(said: &'a str, cwd: &Path) -> &'a str {
    let command = printed_command(said);
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("the printed command runs");
    assert!(status.success(), "`{command}` did not run");
    command
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
    // is where naming the wrong scope would go unnoticed. A directory whose
    // name holds a space is where the value reaches git as a value and a
    // pattern: git stores the first word, exits 0, and the next session prints
    // the identical advice.
    for existing in [None, Some(".husky")] {
        let dir = clone_with(&["pre-commit"]);
        if let Some(value) = existing {
            config(dir.path(), &["core.hooksPath", value]);
        }
        let said = probe(dir.path());
        let command = run_printed(&said, dir.path());
        assert!(
            !probe(dir.path()).contains("are not armed"),
            "`{command}` was printed as the repair and left the clone unarmed"
        );
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let hooks = dir.path().join("my pkg/hooks");
    populate(dir.path(), &hooks, &["pre-commit"]);
    let script = hooks.join("session-start.sh");
    let said = probe_from(dir.path(), &script);
    let command = run_printed(&said, dir.path());
    assert!(
        !probe_from(dir.path(), &script).contains("are not armed"),
        "`{command}` was printed as the repair and left the clone unarmed"
    );
}

#[test]
fn the_arming_command_carries_the_path_as_one_word() {
    // The value reaches git through a shell, so a path is one word there only
    // if it is escaped as one. Unescaped, an ordinary space makes git read a
    // value followed by a pattern — it stores the first word and exits 0 — and
    // a path holding a substitution runs it, in a command the report tells its
    // reader to paste.
    let dir = tempfile::tempdir().expect("tempdir");
    let hooks = dir.path().join("a\'$(touch pwned)\'b/hooks");
    populate(dir.path(), &hooks, &["pre-commit"]);

    let script = hooks.join("session-start.sh");
    let said = probe_from(dir.path(), &script);
    let command = run_printed(&said, dir.path());
    assert!(
        !dir.path().join("pwned").exists(),
        "`{command}` ran what the path spelled"
    );
    assert!(
        !probe_from(dir.path(), &script).contains("are not armed"),
        "`{command}` was printed as the repair and left the clone unarmed"
    );
}

#[test]
fn the_chmod_command_carries_each_path_as_one_word() {
    // The same standard on the other message. Wrapping a path in quotes is not
    // escaping it: a path holding one closes the literal, and what follows is
    // read as shell.
    let dir = tempfile::tempdir().expect("tempdir");
    let hooks = dir.path().join("a\'$(touch pwned)\'b/hooks");
    populate(dir.path(), &hooks, &["pre-commit"]);
    config(
        dir.path(),
        &["core.hooksPath", "a\'$(touch pwned)\'b/hooks"],
    );
    set_mode(&hooks.join("pre-commit"), 0o644);

    let script = hooks.join("session-start.sh");
    let said = probe_from(dir.path(), &script);
    assert!(
        said.contains("executable bit"),
        "the clone is armed and the bit is missing: {said}"
    );
    let command = run_printed(&said, dir.path());
    assert!(
        !dir.path().join("pwned").exists(),
        "`{command}` ran what the path spelled"
    );
    assert_eq!(
        mode_of(&hooks.join("pre-commit")) & 0o111,
        0o111,
        "`{command}` was printed as the repair and left the bit unset"
    );
}

#[test]
fn the_directory_it_names_is_the_one_git_runs_hooks_from() {
    // git rewrites a relative answer to answer to the working directory only
    // while that directory lies inside the work tree. An exported
    // `GIT_WORK_TREE` puts the session outside one, and reading the answer
    // against the working directory then names a directory that does not
    // exist while git runs hooks from another repository entirely.
    let dir = tempfile::tempdir().expect("tempdir");
    let real = dir.path().join("real");
    let decoy = dir.path().join("decoy");
    std::fs::create_dir_all(real.join("githooks")).expect("real hooks");
    populate(&real, &real.join("githooks"), &["pre-commit"]);
    populate(&decoy, &decoy.join("hooks"), &["pre-commit"]);
    config(&real, &["core.hooksPath", "githooks"]);

    let said = probe_with(
        &decoy,
        &decoy.join("hooks/session-start.sh"),
        &[
            ("GIT_DIR", &real.join(".git").display().to_string()),
            ("GIT_WORK_TREE", &real.display().to_string()),
        ],
    );
    assert!(
        said.contains("real/githooks"),
        "git runs hooks from the work tree it was pointed at: {said}"
    );
    assert!(
        !said.contains("decoy/githooks"),
        "no such directory exists, and naming it sends the repair at the wrong tree: {said}"
    );
}

#[test]
fn the_value_it_prints_arms_every_worktree_of_the_clone() {
    // git resolves a relative `core.hooksPath` against each work tree's own
    // root, so the relative form is what carries the repair to a linked
    // worktree — and to the same clone at another path. An absolute value
    // points every one of them at the tree the command happened to run in.
    let dir = clone_with(&["pre-commit"]);
    let main = dir.path();
    assert!(
        common::git(main)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["commit", "-q", "--allow-empty", "-m", "init"])
            .status()
            .expect("git commit runs")
            .success()
    );
    let linked = dir.path().join("linked");
    assert!(
        common::git(main)
            .args(["worktree", "add", "-q", "--detach"])
            .arg(&linked)
            .status()
            .expect("git worktree add runs")
            .success()
    );
    std::fs::create_dir_all(linked.join("hooks")).expect("linked hooks dir");
    std::fs::copy(template(), linked.join("hooks/session-start.sh")).expect("copy probe");
    std::fs::write(linked.join("hooks/pre-commit"), "#!/bin/sh\nexit 1\n").expect("write hook");
    set_mode(&linked.join("hooks/pre-commit"), 0o755);

    let said = probe(main);
    let command = run_printed(&said, main);
    assert!(
        !probe(&linked).contains("are not armed"),
        "`{command}` armed the tree it ran in and left the linked worktree \
         running the hooks of another checkout"
    );
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

#[test]
fn the_chmod_it_prints_runs_where_the_path_carries_a_space() {
    // A path relative to the work tree carries no space the scaffold made, so
    // this reaches the branch that does: hooks kept outside the work tree. An
    // unquoted join hands the operator a command that chmods two paths that do
    // not exist.
    let dir = tempfile::TempDir::with_prefix("probe").expect("tempdir");
    let hooks_dir = dir.path().join("my hooks");
    let work_tree = dir.path().join("repo");
    std::fs::create_dir_all(&work_tree).expect("work tree");
    populate(&work_tree, &hooks_dir, &["pre-commit", "commit-msg"]);
    for hook in ["pre-commit", "commit-msg"] {
        set_mode(&hooks_dir.join(hook), 0o644);
    }
    config(
        &work_tree,
        &["core.hooksPath", &hooks_dir.display().to_string()],
    );

    let script = hooks_dir.join("session-start.sh");
    let said = probe_from(&work_tree, &script);
    assert!(
        printed_command(&said)
            .split_whitespace()
            .any(|word| word.starts_with('/')),
        "this case exists to exercise the absolute fallback: {said}"
    );
    assert_eq!(
        said.matches("my\\ hooks/pre-commit").count(),
        2,
        "the sentence names the paths as well as the command, and a space-joined \
         pair reads there as one path: {said}"
    );
    let command = run_printed(&said, &work_tree);
    assert!(
        !probe_from(&work_tree, &script).contains("executable bit"),
        "`{command}` was printed as the repair and left the bit unset"
    );
}

#[test]
fn the_chmod_it_prints_runs_from_where_the_session_stands() {
    // The hook wrapper enters `CLAUDE_PROJECT_DIR`, which a monorepo puts
    // below the work-tree root. A path relative to that root is the one thing
    // a command read there cannot resolve.
    let dir = tempfile::tempdir().expect("tempdir");
    let project = dir.path().join("pkg");
    let hooks = project.join("hooks");
    populate(dir.path(), &hooks, &["pre-commit"]);
    config(dir.path(), &["core.hooksPath", "pkg/hooks"]);
    set_mode(&hooks.join("pre-commit"), 0o644);

    let script = hooks.join("session-start.sh");
    let said = probe_from(&project, &script);
    assert!(
        said.contains("executable bit"),
        "the clone is armed and the bit is missing: {said}"
    );
    let command = run_printed(&said, &project);
    assert_eq!(
        mode_of(&hooks.join("pre-commit")) & 0o111,
        0o111,
        "`{command}` is read from the project directory, not from the work-tree root"
    );
}

#[test]
fn a_git_that_cannot_answer_the_question_is_not_answered_for() {
    // A git predating `--git-path` echoes the flag onto stdout, reads the
    // operand as the `hooks` directory beside it, and still exits 0. Read as a
    // path, that answer tells every session the clone is unarmed when it is
    // not — and names a repair that changes nothing.
    let dir = clone_with(&["pre-commit"]);
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).expect("shim dir");
    let real = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|d| Path::new(d).join("git"))
        .find(|p| p.is_file())
        .expect("no git on PATH");
    std::fs::write(
        bin.join("git"),
        format!(
            "#!/bin/sh\ncase \" $* \" in\n  *\" --git-path \"*) \
             printf -- '--git-path\\nhooks\\n'; exit 0 ;;\nesac\nexec {} \"$@\"\n",
            real.display()
        ),
    )
    .expect("write shim");
    set_mode(&bin.join("git"), 0o755);

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let said = probe_with(
        dir.path(),
        &dir.path().join("hooks/session-start.sh"),
        &[("PATH", &path)],
    );
    assert!(
        !said.contains("are not armed") && !said.contains("--git-path"),
        "an answer that is the question is not a directory: {said}"
    );
    assert!(
        probe(dir.path()).contains("are not armed"),
        "the same clone under a git that can answer is reported"
    );
}

#[test]
fn a_repository_whose_path_holds_a_newline_is_still_reported() {
    // The answer's shape cannot stand in for the check above: a repository
    // path holding a newline gives a two-line answer that is a path. Rejecting
    // it by shape leaves a genuinely unarmed clone silent — the one state this
    // probe exists to break.
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join(std::ffi::OsStr::from_bytes(b"nl\ndir"));
    std::fs::create_dir_all(&root).expect("newline dir");
    let elsewhere = root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("elsewhere");
    populate(&root, &root.join("hooks"), &["pre-commit"]);
    config(&root, &["core.hooksPath", &elsewhere.display().to_string()]);

    let said = probe(&root);
    assert!(
        said.contains("are not armed"),
        "git runs hooks from elsewhere, whatever the path spells: {said}"
    );
}
