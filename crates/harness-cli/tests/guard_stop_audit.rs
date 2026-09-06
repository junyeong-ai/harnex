//! What `harnex guard stop-audit` puts on each of the Stop event's channels.
//!
//! The channel is the contract, because the event has three and they differ by
//! reader: `systemMessage` for the operator, `hookSpecificOutput` for the
//! model, and stderr for the reason a blocked stop is given. Only which one
//! each outcome lands on is asserted here — the decision behind it is
//! `guard::stop_audit`'s own subject, and its runner seam covers it without
//! spawning anything.
//!
//! Every case reaches its outcome before the critique spawn, so no model call
//! is made: an unanswerable probe skips, a probe answering "no work" allows,
//! and a retry counter already past its ceiling blocks.

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

fn stop_audit_in(dir: &Path, config: &str) -> Output {
    std::fs::write(dir.join("harness.toml"), config).expect("write harness.toml");
    Command::new(env!("CARGO_BIN_EXE_harnex"))
        .args(["guard", "stop-audit", "--session", "contract"])
        .current_dir(dir)
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
        &harness_toml(
            "[guard.stop_audit]\ncritique_skill = \"/unreachable\"\n\
             has_changes_check = [\"true\"]\n",
        ),
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
fn a_block_feeds_back_where_exit_two_is_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    // The ceiling is already reached, so the escalation lands before the
    // critique is spawned — the only branch that reaches a Blocker without
    // paying for a model call.
    let ledger = dir.path().join(".harness/_audit_retry");
    std::fs::create_dir_all(&ledger).expect("retry ledger dir");
    std::fs::write(ledger.join("contract.count"), "1").expect("seed the counter");
    let output = stop_audit_in(
        dir.path(),
        &harness_toml(
            "[guard.stop_audit]\ncritique_skill = \"/unreachable\"\n\
             max_retries = 1\nhas_changes_check = [\"false\"]\n",
        ),
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "exit 2 is what prevents the stop"
    );
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr is utf-8");
    assert!(
        stderr.contains("retry counter exceeded"),
        "on exit 2 the runtime builds the block reason from stderr alone: {stderr}"
    );
    assert_eq!(
        stdout_of(&output),
        "",
        "stdout is ignored on exit 2, so a reason written there is a reason lost"
    );
}
