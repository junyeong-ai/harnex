//! What `harnex guard stop-audit` puts on each of the Stop event's channels.
//!
//! The shape is the whole contract: a Stop hook's `systemMessage` reaches the
//! transcript's raw stdout and no reader, so an advisory written there is
//! indistinguishable from one never written. Only the channel each outcome
//! lands on is asserted here — the decision behind it is `guard::stop_audit`'s
//! own subject, and its runner seam covers it without spawning anything.
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
fn a_skip_rides_the_stop_channel_that_arrives() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = stop_audit_in(dir.path(), &harness_toml(""));

    assert_eq!(output.status.code(), Some(0), "a skip allows the stop");
    let body: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("stdout is one JSON object");
    assert_eq!(
        body["hookSpecificOutput"]["hookEventName"], "Stop",
        "the event names itself, which the runtime requires to route the field"
    );
    let context = body["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("additionalContext carries the reason");
    assert!(
        context.contains("no [guard.stop_audit] section"),
        "the skip says why it could not judge: {context}"
    );
    assert!(
        body.get("systemMessage").is_none(),
        "a Stop systemMessage reaches no reader, so writing one would be silence"
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
