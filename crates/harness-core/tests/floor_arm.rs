//! What the shipped PreToolUse arm does with an oracle it cannot use.
//!
//! A PreToolUse exit of 2 blocks the agent's action, and it is also what an
//! argument parser exits on, so the arm stands between the two: a project
//! whose oracle predates this subcommand would otherwise have every command
//! it ran refused. The arm is exercised through the template, which is what
//! adopters receive; this repository's copy is held byte-identical to it by
//! `adopted_scaffold_matches_templates`.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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

/// What the runtime writes to a PreToolUse hook, for the cases that care what
/// reaches the far side of the arm.
const TOOL_CALL: &str =
    r#"{"tool_name":"Bash","tool_input":{"command":"git commit --no-verify -m x"}}"#;

fn run_with_stdin(path: &str, cwd: &Path, input: &str) -> Output {
    let mut child = Command::new(bash())
        .arg(arm())
        .current_dir(cwd)
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run the arm");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(input.as_bytes())
        .expect("write the tool call");
    child.wait_with_output().expect("the arm finishes")
}

#[test]
fn an_absent_oracle_leaves_the_tool_call_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = run("", dir.path());
    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stdout.is_empty() && output.stderr.is_empty(),
        "and says nothing about it"
    );
}

#[test]
fn an_oracle_without_this_subcommand_leaves_the_tool_call_alone() {
    // What clap exits on an unrecognised subcommand is 2 — the block code. An
    // oracle this old still answers a bare `--help`, which is what separates
    // it from one that does not run.
    let (dir, path) = with_oracle(
        "if [ \"${1:-}\" = --help ]; then exit 0; fi\n\
         echo \"error: unrecognized subcommand\" >&2; exit 2",
    );
    let output = run(&path, dir.path());
    assert_eq!(
        output.status.code(),
        Some(0),
        "a parser's usage error is not a floor violation"
    );
    assert!(
        output.stdout.is_empty() && output.stderr.is_empty(),
        "and says nothing: an oracle predating the subcommand is the ordinary \
         state this arm exists for, not a fault to report on every tool call"
    );
}

/// The state `command -v` cannot tell from a working oracle: a version
/// manager's shim with no version selected, a dangling symlink, an install cut
/// short. The floor is down and the operator has every reason to believe it is
/// standing, so this is the one skip the arm spends a notice on.
///
/// On stdout, as the control JSON the runtime reads from a hook that exits 0.
/// A hook's stderr is read only where it exits 2, so the same words written
/// there would leave the skip as silent as no notice at all.
#[test]
fn an_oracle_that_does_not_run_says_so_on_the_operators_channel() {
    let (dir, path) = with_oracle("exit 1");
    let output = run(&path, dir.path());
    assert_eq!(
        output.status.code(),
        Some(0),
        "an unusable oracle is still not a floor violation"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let notice: serde_json::Value =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("{stdout:?}: {e}"));
    let message = notice["systemMessage"].as_str().unwrap_or_default();
    assert!(
        message.contains("floor-check skipped"),
        "silence here reads as a standing floor: {stdout}"
    );
    assert!(
        message.contains(&path),
        "and names which binary, since the operator has more than one: {stdout}"
    );
}

/// A path is interpolated into that JSON, and a directory name may hold any
/// byte but `/` and NUL. Malformed control output is discarded by the runtime,
/// which would make the notice silent exactly where it is needed.
#[test]
fn a_path_that_could_break_the_json_does_not() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Both characters that end a JSON string, and control characters from
    // either side of the ones with a short escape.
    let bin = dir.path().join("say \"hi\"\\ok\u{1}and\tthen\rmore");
    std::fs::create_dir_all(&bin).expect("bin");
    let stub = bin.join("harnex");
    std::fs::write(&stub, "#!/bin/sh\nexit 1\n").expect("write stub");
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let output = run(&bin.display().to_string(), dir.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let notice: serde_json::Value =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("{stdout:?}: {e}"));
    let message = notice["systemMessage"].as_str().expect("a string");
    assert!(
        message.contains("say \"hi\"\\ok"),
        "what only needs escaping survives it: {message:?}"
    );
    assert!(
        !message.chars().any(|c| (c as u32) < 0x20),
        "what a string body cannot carry at all is gone: {message:?}"
    );
}

/// Both probes run before the oracle does, and the oracle reads the tool call
/// off stdin. A probe that consumed it, or a redirection that replaced it,
/// would hand the floor an empty command — and a floor shown no command has
/// nothing to refuse.
#[test]
fn the_probes_leave_stdin_for_the_oracle() {
    // A real oracle answers either `--help` from its parser without reading
    // stdin. This one does the same, then reports what reached it: 2 where the
    // tool call arrived whole, 3 where stdin was already at end. Shell
    // builtins only — a case runs with a `PATH` that answers for the oracle
    // and for nothing else.
    // What it read, not whether `read` returned true: a tool call carries no
    // trailing newline, so `read` reports the end of input having filled the
    // variable, and a status test here would fail on a stdin that arrived.
    let (dir, path) = with_oracle(
        "case \"${1:-} ${3:-}\" in *--help*) exit 0 ;; esac\n\
         read -r line\n\
         case \"$line\" in *--no-verify*) exit 2 ;; \"\") exit 3 ;; *) exit 4 ;; esac",
    );
    let output = run_with_stdin(&path, dir.path(), TOOL_CALL);
    assert_eq!(
        output.status.code(),
        Some(2),
        "the oracle read the whole tool call off stdin; 3 is a stdin already \
         spent by a probe, 4 is one that arrived altered"
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
