//! Stop wrappers never request continuation or mix child output into their envelope.

use std::process::Command;

#[test]
fn a_missing_stop_program_reports_failure_without_requesting_continuation() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_harnex"))
        .args(["guard", "hook-stop"])
        .arg(root.path().join("missing-program"))
        .env("CLAUDE_PROJECT_DIR", root.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"]["code"], "GUARD_SPAWN_FAILURE");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("skipped")
    );
}

#[cfg(unix)]
#[test]
fn stop_program_output_is_advisory_and_stdout_stays_one_envelope() {
    for code in [0, 2] {
        let root = tempfile::tempdir().unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_harnex"))
            .args(["guard", "hook-stop", "/bin/sh", "-c"])
            .arg(format!(
                "printf '%s\\n' '{{\"decision\":\"block\"}}'; exit {code}"
            ))
            .env("CLAUDE_PROJECT_DIR", root.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(body["data"]["outcome"], "stop-forced-success");
        assert_eq!(body["data"]["observed_exit_code"], code);
        assert!(body.get("decision").is_none());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("\"decision\":\"block\"")
        );
    }
}
