//! What `harnex guard floor` puts on the PreToolUse event's channels.
//!
//! The exit code is the contract: 2 blocks the agent's action with the reason
//! on stderr, and everything else allows — a skip saying why on
//! `systemMessage`, an allowance saying nothing at all. Inability to evaluate
//! is not a violation, so no path here may reach 2 without one.
//!
//! The two controls are wired separately because their costs differ, and this
//! file is where that separation is computed rather than promised: the
//! tripwire stands over a project that declares no floor, and the freeze says
//! so instead of pretending to hold.

use std::path::Path;
use std::process::{Command, Output};

fn harness_toml(floor: &str) -> String {
    format!(
        "[meta]\nharnex_version = \"{}\"\n{floor}",
        concat!(
            ">=",
            env!("CARGO_PKG_VERSION_MAJOR"),
            ".",
            env!("CARGO_PKG_VERSION_MINOR")
        )
    )
}

fn floor_in(dir: &Path, config: &str, tool: &str, tool_input: serde_json::Value) -> Output {
    std::fs::write(dir.join("harness.toml"), config).expect("write harness.toml");
    floor_at(dir, tool, tool_input)
}

fn floor_at(dir: &Path, tool: &str, tool_input: serde_json::Value) -> Output {
    std::fs::create_dir_all(dir.join(".git")).expect("a repository root");
    let payload = serde_json::json!({
        "session_id": "contract",
        "transcript_path": "/dev/null",
        "cwd": dir.display().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": tool,
        "tool_input": tool_input,
    })
    .to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_harnex"))
        .args(["guard", "floor"])
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run harnex");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.as_bytes())
        .expect("write the hook event");
    child.wait_with_output().expect("collect output")
}

fn bash(command: &str) -> serde_json::Value {
    serde_json::json!({ "command": command })
}

fn write(file_path: &str) -> serde_json::Value {
    serde_json::json!({ "file_path": file_path })
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

#[test]
fn a_hook_skipping_command_is_refused_with_no_floor_declared() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = floor_in(
        dir.path(),
        &harness_toml(""),
        "Bash",
        bash("git commit --no-verify -m x"),
    );
    assert_eq!(output.status.code(), Some(2), "exit 2 blocks the action");
    let reason = text(&output.stderr);
    assert!(reason.contains("--no-verify"), "{reason}");
    assert!(
        text(&output.stdout).is_empty(),
        "a block speaks on stderr, not the operator's channel"
    );
}

#[test]
fn an_ordinary_command_says_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = floor_in(dir.path(), &harness_toml(""), "Bash", bash("cargo test"));
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stdout).is_empty(), "an allowance is silent");
}

#[test]
fn a_write_with_no_floor_declared_is_allowed_and_says_why() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = floor_in(
        dir.path(),
        &harness_toml(""),
        "Write",
        write("harness.toml"),
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "a floor nobody declared freezes nothing"
    );
    let body: serde_json::Value =
        serde_json::from_str(text(&output.stdout).trim()).expect("one JSON object");
    let message = body["systemMessage"].as_str().expect("the skip says why");
    assert!(message.contains("[guard.floor]"), "{message}");
}

#[test]
fn a_hook_skipping_command_is_refused_with_no_configuration_at_all() {
    // `harness.toml` is frozen against the Edit tools but not against Bash, so
    // a configuration the tripwire needed would be one `rm` away from taking
    // the tripwire with it.
    let dir = tempfile::tempdir().expect("tempdir");
    let output = floor_at(dir.path(), "Bash", bash("git commit --no-verify -m x"));
    assert_eq!(output.status.code(), Some(2), "exit 2 blocks the action");
    assert!(text(&output.stderr).contains("--no-verify"));

    let write = floor_at(dir.path(), "Write", write("harness.toml"));
    assert_eq!(
        write.status.code(),
        Some(0),
        "the freeze has nothing to read, so it freezes nothing"
    );
    let body: serde_json::Value =
        serde_json::from_str(text(&write.stdout).trim()).expect("one JSON object");
    let message = body["systemMessage"].as_str().expect("the skip says why");
    assert!(message.contains("harness.toml"), "{message}");
}

#[test]
fn a_declared_floor_freezes_what_it_names_and_what_is_built_in() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = harness_toml("\n[guard.floor]\nprotected_paths = [\"hooks/\"]\n");
    for path in ["harness.toml", ".claude/settings.json", "hooks/pre-commit"] {
        let output = floor_in(dir.path(), &config, "Write", write(path));
        assert_eq!(output.status.code(), Some(2), "{path} is frozen");
        assert!(text(&output.stderr).contains(path), "{path}");
    }
    let free = floor_in(dir.path(), &config, "Write", write("src/main.rs"));
    assert_eq!(
        free.status.code(),
        Some(0),
        "an ordinary file is not frozen"
    );
}

#[test]
fn stdin_that_is_not_a_hook_event_allows_and_says_so() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("harness.toml"), harness_toml("")).expect("write");
    let mut child = Command::new(env!("CARGO_BIN_EXE_harnex"))
        .args(["guard", "floor"])
        .current_dir(dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("run harnex");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"not json")
        .expect("write");
    let output = child.wait_with_output().expect("collect output");
    assert_eq!(
        output.status.code(),
        Some(0),
        "input this binary cannot read is not a violation"
    );
    assert!(text(&output.stdout).contains("floor-check skipped"));
}
