//! End-to-end CLI contract tests: every invocation — including a malformed
//! one — honors the envelope + exit-code contract.

use std::process::Command;

use harness_core::guard::{OPERATOR_CHANNEL_KEY, SUPPRESS_OUTPUT_KEY};

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

/// A `guard` invocation is a hook, and the runtime reads its exit 2 as a
/// Block: an arm this binary cannot parse skips on the operator's channel
/// instead of refusing every tool call or keeping a session from stopping.
///
/// The notice is all the operator learns of an arm a newer binary dropped, so
/// it names what did not parse.
#[test]
fn a_guard_invocation_that_does_not_parse_skips_instead_of_blocking() {
    for (args, named) in [
        (&["guard", "no-such-verb"][..], "'no-such-verb'"),
        (&["guard", "floor", "--no-such-flag"], "--no-such-flag"),
        (&["guard"], "a subcommand is required"),
    ] {
        let out = harness().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        let json: serde_json::Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: stdout is one JSON object: {e}"));
        assert!(
            json[OPERATOR_CHANNEL_KEY]
                .as_str()
                .is_some_and(|m| m.contains("skipped") && m.contains(named)),
            "{args:?}: {json}"
        );
        assert_eq!(json[SUPPRESS_OUTPUT_KEY], true, "{args:?}");
    }
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
    assert!(
        message.contains("<TARGET>"),
        "names the argument: {message}"
    );
    assert!(message.contains("'bogus'"), "names the value: {message}");
    assert!(
        message.contains("session-trend"),
        "names the valid set: {message}"
    );

    // An option with no closed set names none, rather than an empty one.
    let out = harness()
        .args(["lifecycle", "classify", "--kind"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("one JSON envelope");
    let message = json["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("--kind") && !message.contains("expected one of"),
        "{message}"
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
