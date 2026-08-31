//! End-to-end CLI contract tests: every invocation — including a malformed
//! one — honors the envelope + exit-code contract.

use std::process::Command;

fn harness() -> Command {
    Command::new(env!("CARGO_BIN_EXE_harnex"))
}

#[test]
fn invalid_argument_emits_error_envelope_and_exit_2() {
    // A malformed invocation must NOT fall back to clap's bare stderr — it
    // emits one JSON error envelope on stdout and exits 2.
    let out = harness().arg("--definitely-not-a-flag").output().unwrap();
    assert_eq!(out.status.code(), Some(2), "invalid args must exit 2");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be one JSON envelope, got {stdout:?}: {e}"));
    assert_eq!(json["ok"], serde_json::Value::Bool(false));
    assert!(json["error"]["code"].is_string());
}

#[test]
fn unknown_subcommand_emits_error_envelope_and_exit_2() {
    let out = harness().arg("nonexistent-subcommand").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("one JSON envelope");
    assert_eq!(json["ok"], serde_json::Value::Bool(false));
}

#[test]
fn invalid_value_envelope_names_the_value_and_the_valid_set() {
    // The envelope is the programmatic surface, so a rejected value must be
    // diagnosable from it alone: which argument, which value, which values
    // exist. clap's kind alone says none of the three.
    let out = harness()
        .args(["export", "schema", "bogus"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("one JSON envelope");
    let message = json["error"]["message"].as_str().unwrap();
    assert!(message.contains("'bogus'"), "names the value: {message}");
    assert!(
        message.contains("session-trend"),
        "names the valid set: {message}"
    );
}

#[test]
fn help_is_clap_native_and_exits_0() {
    // `--help` is a display request, not a command execution — clap-native,
    // exit 0, NOT enveloped.
    let out = harness().arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0), "--help must exit 0");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("Usage") || stdout.contains("harnex"),
        "help text expected, got: {stdout:?}"
    );
}

#[test]
fn version_is_clap_native_and_exits_0() {
    let out = harness().arg("--version").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
}
