//! What the shipped hook wrapper does when it cannot reach a verifier.
//!
//! Every one of those states ends the hook at 0, because a broken toolchain
//! must never block an edit. That makes the notice the whole of what the
//! operator gets, and a hook exiting 0 is read for the control JSON on its
//! stdout — its stderr is read only at exit 2. A skip written to stderr here
//! is a verifier that quietly did not run.
//!
//! The wrapper resolves its own directory to find verifiers, so each case
//! copies the template into a directory it controls and runs that copy.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn template() -> PathBuf {
    root().join("plugins/harnex/templates/common/_runner.sh")
}

/// The wrapper, placed in a directory of this case's own, with `verifiers`
/// written beside it as the wrapper expects to find them.
fn hooks_dir(verifiers: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let arm = dir.path().join("_runner.sh");
    std::fs::copy(template(), &arm).expect("copy the wrapper");
    std::fs::set_permissions(&arm, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    for name in verifiers {
        let path = dir.path().join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write verifier");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    dir
}

/// Resolved here, because a case may run the wrapper with a `PATH` that
/// answers for nothing.
fn bash() -> PathBuf {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|d| Path::new(d).join("bash"))
        .find(|p| p.is_file())
        .expect("no bash on PATH")
}

fn run(hooks: &Path, args: &[&str], env: BTreeMap<&str, String>) -> Output {
    let mut command = Command::new(bash());
    command.arg(hooks.join("_runner.sh")).args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("run the wrapper")
}

fn project_dir(path: &Path) -> BTreeMap<&'static str, String> {
    BTreeMap::from([("CLAUDE_PROJECT_DIR", path.display().to_string())])
}

/// Exit 0 with the reason on the operator's channel, and nothing on stderr.
fn assert_skips_saying(output: &Output, expected: &str) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "a wrapper that cannot dispatch never blocks the edit"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let notice: serde_json::Value =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("{stdout:?}: {e}"));
    let message = notice["systemMessage"].as_str().unwrap_or_default();
    assert!(
        message.contains(expected),
        "expected a skip naming {expected:?}, got {stdout:?}"
    );
    assert!(
        output.stderr.is_empty(),
        "and nothing on a channel an exit of 0 does not carry: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_relative_project_root_is_refused() {
    let hooks = hooks_dir(&[]);
    let env = BTreeMap::from([("CLAUDE_PROJECT_DIR", "relative/path".to_string())]);
    let output = run(hooks.path(), &["post-format.sh"], env);
    assert_skips_saying(&output, "project root is not an absolute path");
}

#[test]
fn a_project_root_that_cannot_be_entered_is_refused() {
    let hooks = hooks_dir(&[]);
    let env = BTreeMap::from([("CLAUDE_PROJECT_DIR", "/nonexistent/root".to_string())]);
    let output = run(hooks.path(), &["post-format.sh"], env);
    assert_skips_saying(&output, "cannot enter project root");
}

#[test]
fn no_script_argument_is_refused() {
    let hooks = hooks_dir(&[]);
    let output = run(hooks.path(), &[], project_dir(hooks.path()));
    assert_skips_saying(&output, "no script argument");
}

#[test]
fn a_traversing_verifier_name_is_refused() {
    let hooks = hooks_dir(&[]);
    let output = run(hooks.path(), &["../escape.sh"], project_dir(hooks.path()));
    assert_skips_saying(&output, "path traversal refused");
}

#[test]
fn a_verifier_that_is_not_there_is_refused() {
    let hooks = hooks_dir(&[]);
    let output = run(hooks.path(), &["absent.sh"], project_dir(hooks.path()));
    assert_skips_saying(&output, "verifier not found");
}

#[test]
fn an_extension_no_arm_dispatches_is_refused() {
    let hooks = hooks_dir(&["check.rb"]);
    let output = run(hooks.path(), &["check.rb"], project_dir(hooks.path()));
    assert_skips_saying(&output, "unsupported verifier extension");
}

/// A `PATH` with the base utilities and none of the interpreters, so what the
/// arm probes for is the only thing missing.
const WITHOUT_INTERPRETERS: &str = "/usr/bin:/bin";

#[test]
fn a_missing_interpreter_is_refused_rather_than_run() {
    let hooks = hooks_dir(&["check.ts"]);
    let mut env = project_dir(hooks.path());
    env.insert("PATH", WITHOUT_INTERPRETERS.to_string());
    let output = run(hooks.path(), &["check.ts"], env);
    assert_skips_saying(&output, "node not found");
}

#[test]
fn a_missing_python_environment_is_refused_rather_than_run() {
    let hooks = hooks_dir(&["check.py"]);
    let mut env = project_dir(hooks.path());
    env.insert("PATH", WITHOUT_INTERPRETERS.to_string());
    let output = run(hooks.path(), &["check.py"], env);
    assert_skips_saying(&output, "uv env unavailable");
}

/// The wrapper finds its own directory before it can report anything, and it
/// does so without running a command. A `PATH` answering for nothing still
/// leaves it anchored beside its verifiers: reaching the interpreter probe is
/// what says it found one. Resolving through `dirname` instead put it on
/// whatever directory the hook fired in, and said so to no one.
#[test]
fn the_wrapper_places_itself_without_running_a_command() {
    let hooks = hooks_dir(&["check.ts"]);
    let mut env = project_dir(hooks.path());
    env.insert("PATH", "/nonexistent/bin".to_string());
    let output = run(hooks.path(), &["check.ts"], env);
    assert_skips_saying(&output, "node not found");
}

/// The shell arm is the one the wrapper probes nothing for, so it is also the
/// case that proves the others are not simply refusing everything.
#[test]
fn a_shell_verifier_is_dispatched() {
    let hooks = hooks_dir(&["post-format.sh"]);
    let output = run(hooks.path(), &["post-format.sh"], project_dir(hooks.path()));
    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stdout.is_empty() && output.stderr.is_empty(),
        "a verifier that ran says nothing of the wrapper's: {:?} {:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A verifier's own exit code is the hook's — the wrapper stands aside once it
/// has something to dispatch to.
#[test]
fn a_failing_verifier_reaches_the_runtime_intact() {
    let dir = hooks_dir(&[]);
    let arm = dir.path().join("fail.sh");
    std::fs::write(&arm, "#!/bin/sh\necho 'the reason' >&2\nexit 2\n").expect("write");
    std::fs::set_permissions(&arm, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let output = run(dir.path(), &["fail.sh"], project_dir(dir.path()));
    assert_eq!(output.status.code(), Some(2), "the verifier's own code");
    assert!(String::from_utf8_lossy(&output.stderr).contains("the reason"));
}
