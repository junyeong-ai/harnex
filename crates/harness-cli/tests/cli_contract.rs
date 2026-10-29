//! End-to-end CLI contract tests: every invocation — including a malformed
//! one — honors the envelope + exit-code contract.

use std::process::Command;

fn harness() -> Command {
    Command::new(env!("CARGO_BIN_EXE_harnex"))
}

/// `hook-run` anchors on the runtime's project, not on the directory the hook
/// fired in. Spawned rather than called in-process, because the precedence is
/// read from the environment and mutating that inside a test process races
/// every other test.
#[cfg(unix)]
#[test]
fn hook_run_anchors_on_the_runtime_project_over_the_working_directory() {
    let outer = tempfile::tempdir().unwrap();
    let inner = outer.path().join("vendor/dep");
    std::fs::create_dir_all(&inner).unwrap();
    for dir in [outer.path(), inner.as_path()] {
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(dir)
                .status()
                .unwrap()
                .success()
        );
    }

    // Fired from the nested checkout: without the variable the probe answers
    // the inner repository, so the runtime's answer is what must win.
    let ran_at = |env: Option<&std::path::Path>| {
        let mut cmd = harness();
        cmd.args(["guard", "hook-run", "pwd"]).current_dir(&inner);
        match env {
            Some(root) => cmd.env("CLAUDE_PROJECT_DIR", root),
            None => cmd.env_remove("CLAUDE_PROJECT_DIR"),
        };
        let out = cmd.output().unwrap();
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    };

    let (_, with_runtime) = ran_at(Some(outer.path()));
    assert!(
        with_runtime.contains(outer.path().file_name().unwrap().to_str().unwrap())
            && !with_runtime.contains("vendor/dep"),
        "the runtime's project must win over the nested checkout: {with_runtime}"
    );

    let (_, from_probe) = ran_at(None);
    assert!(
        from_probe.contains("vendor/dep"),
        "with no runtime to ask, the probe still answers where it is run: {from_probe}"
    );

    // Set but unusable decides too — never a quiet fall-through to the probe,
    // which would gate the inner repository.
    let bogus = outer.path().join("does-not-exist");
    let mut cmd = harness();
    let out = cmd
        .args(["guard", "hook-run", "pwd"])
        .current_dir(&inner)
        .env("CLAUDE_PROJECT_DIR", &bogus)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("skipped-fail-open"),
        "a runtime project that cannot be entered must skip, not run somewhere else: {stdout}"
    );
    assert!(
        !stdout.contains("vendor/dep"),
        "the inner repository must never be the answer: {stdout}"
    );
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
    assert!(
        message.contains("<TARGET>"),
        "names the argument: {message}"
    );
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
