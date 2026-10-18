//! What `harnex guard stop-audit` puts on each of the Stop event's channels.
//!
//! The channel is the contract, because the event has three and they differ by
//! reader: `systemMessage` for the operator, `hookSpecificOutput` for the
//! model, and stderr for the reason a blocked stop is given. Only which one
//! each outcome lands on is asserted here — the decision behind it is
//! `guard::stop_audit`'s own subject, and its runner seam covers it without
//! spawning anything.
//!
//! A critique, where needed, is a local stub. No test invokes a model.

use std::path::Path;
use std::process::{Command, Output};

fn harness_toml(stop_audit: &str) -> String {
    format!(
        "[meta]\nharnex_version = \"{}\"\n{stop_audit}",
        concat!(
            ">=",
            env!("CARGO_PKG_VERSION_MAJOR"),
            ".",
            env!("CARGO_PKG_VERSION_MINOR")
        )
    )
}

/// The probe's program, resolved here so the child needs no `PATH`.
fn resolved(program: &str) -> String {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|d| Path::new(d).join(program))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("no {program} on PATH"))
        .display()
        .to_string()
}

/// Runs with an empty `PATH`, which is what makes the no-model-call promise a
/// computed one: every case here reaches its outcome before `spawn_critique`,
/// and a case that stopped doing so would look for `claude`, fail to find it,
/// and take the skip branch — failing this file's assertions rather than
/// quietly paying for a turn.
///
/// An empty `PATH` still searches the working directory on some platforms, so
/// the run happens in a fresh temporary one holding only what a case wrote.
/// Pointing this at a populated directory would put whatever it holds back on
/// the search path.
fn stop_audit_in(dir: &Path, config: &str) -> Output {
    std::fs::write(dir.join("harness.toml"), config).expect("write harness.toml");
    Command::new(env!("CARGO_BIN_EXE_harnex"))
        .args(["guard", "stop-audit", "--session", "contract"])
        .current_dir(dir)
        .env("PATH", "")
        .output()
        .expect("run harnex")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is utf-8")
}

#[test]
fn a_skip_reaches_the_operator_who_has_to_fix_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = stop_audit_in(dir.path(), &harness_toml(""));

    assert_eq!(output.status.code(), Some(0), "a skip allows the stop");
    let body: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("stdout is one JSON object");
    let message = body["systemMessage"]
        .as_str()
        .expect("the operator's channel carries the reason");
    assert!(
        message.contains("no [guard.stop_audit] section"),
        "the skip says why it could not judge: {message}"
    );
    assert!(
        body.get("hookSpecificOutput").is_none(),
        "a section this binary cannot load is not something the model can act on, \
         so it does not reach the model's channel"
    );
}

#[test]
fn an_allowed_stop_says_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = stop_audit_in(
        dir.path(),
        &harness_toml(&format!(
            "[guard.stop_audit]\ncritique_skill = \"/unreachable\"\n\
             retry_ledger_dir = \"retry\"\nhas_changes_check = [\"{}\"]\n",
            resolved("true")
        )),
    );

    assert_eq!(output.status.code(), Some(0), "nothing to critique allows");
    assert_eq!(
        stdout_of(&output),
        "",
        "an allowance with nothing to say says nothing, rather than a body \
         whose keys the runtime reports as unrecognised"
    );
}

#[test]
fn a_spent_budget_notifies_the_operator_and_allows_the_stop() {
    let dir = tempfile::tempdir().expect("tempdir");
    // The ceiling is already reached, so the escalation lands before the
    // critique is spawned. The ledger directory is declared rather than left to
    // its default: a seed written to a path the binary no longer reads would
    // start the counter at one, fall through to the spawn, and pay for the turn
    // this case exists to avoid.
    let ledger = dir.path().join("retry");
    std::fs::create_dir_all(&ledger).expect("retry ledger dir");
    std::fs::write(ledger.join("contract.count"), "1").expect("seed the counter");
    let output = stop_audit_in(
        dir.path(),
        &harness_toml(&format!(
            "[guard.stop_audit]\ncritique_skill = \"/unreachable\"\n\
             retry_ledger_dir = \"retry\"\nmax_retries = 1\n\
             has_changes_check = [\"{}\"]\n",
            resolved("false")
        )),
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "escalation must end automatic continuation"
    );
    let body: serde_json::Value = serde_json::from_str(&stdout_of(&output)).unwrap();
    assert!(
        body["systemMessage"]
            .as_str()
            .unwrap()
            .contains("operator review is required")
    );
    assert!(body.get("hookSpecificOutput").is_none());
    assert!(output.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn critique_results_reach_their_reader_without_a_model_call() {
    use std::os::unix::fs::PermissionsExt;
    for (code, response, blocked, silent) in [
        (
            0,
            r#"{"ok":true,"data":{"items":[{"severity":"major"}]}}"#,
            true,
            false,
        ),
        (0, r#"{"ok":true,"data":{"items":[]}}"#, false, true),
        (
            0,
            r#"{"ok":true,"data":{"items":[{}, {"severity":"blocker"}]}}"#,
            true,
            false,
        ),
        (1, r#"{"ok":true,"data":{"items":[]}}"#, false, false),
        (0, "unreadable", false, false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("claude");
        std::fs::write(
            &stub,
            format!("#!/bin/sh\nprintf '%s\\n' '{response}'\nexit {code}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            dir.path().join("harness.toml"),
            harness_toml(&format!(
                "[guard.stop_audit]\ncritique_skill=\"/critique\"\nhas_changes_check=[\"{}\"]\n",
                resolved("false")
            )),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_harnex"))
            .args(["guard", "stop-audit", "--session", "contract"])
            .current_dir(dir.path())
            .env("PATH", dir.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(if blocked { 2 } else { 0 }));
        if blocked {
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains("gating findings")
            );
            assert!(output.stdout.is_empty());
        } else if silent {
            assert!(output.stdout.is_empty());
            assert!(output.stderr.is_empty());
        } else {
            let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(
                body["systemMessage"]
                    .as_str()
                    .unwrap()
                    .contains("stop-audit skipped")
            );
            assert!(output.stderr.is_empty());
        }
    }
}
