//! End-to-end CLI contract tests: every invocation — including a malformed
//! one — honors the envelope + exit-code contract, and answers the same
//! whatever its caller exports about git pathspecs.

use std::path::{Path, PathBuf};
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

/// Git's pathspec variables, exported in the caller's shell, would reread the
/// pathspecs harnex hands git: a committed rule's age reads as unrecorded, a
/// `claude.md` joins the memory a `CLAUDE.md` names, and a declared harness
/// path is read as a file's name or with its `*` stopping at a `/`, so a
/// baseline records a harness older than the one the window ran under.
#[test]
fn the_callers_pathspec_variables_change_no_answer() {
    let project = tempfile::tempdir().unwrap();
    let transcripts = tempfile::tempdir().unwrap();
    let root = project.path();
    // A finding and a session name the project as the system resolves it.
    let seen_root = root.canonicalize().unwrap();
    let git = |args: &[&str], date: &str| {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    let write = |path: &str, body: &str| {
        std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        std::fs::write(root.join(path), body).unwrap();
    };
    let commit = |date: &str| {
        git(&["add", "."], date);
        let args = ["-c", "user.name=t", "-c", "user.email=t@t"];
        git(&[&args[..], &["commit", "-qm", "x"]].concat(), date);
        git(&["rev-parse", "HEAD"], date)
    };
    let ten_days_ago = format!(
        "{} +0000",
        (jiff::Timestamp::now() - jiff::SignedDuration::from_hours(240)).as_second()
    );
    git(&["init", "-q"], &ten_days_ago);
    git(&["config", "core.ignorecase", "false"], &ten_days_ago);
    write(
        "harness.toml",
        &format!(
            "[meta]\nharnex_version = \">={}\"\n[evidence]\n[[kinds]]\nname = \"rule\"\n\
             glob = \".claude/rules/*.md\"\n[lifecycle]\n[[lifecycle.consumer_detectors]]\n\
             kind = \"rule\"\npattern = \"{{slug}}\"\n[session]\nroots = [{}]\n\
             harness_paths = [\"**/CLAUDE.md\", \"docs/*.md\"]\n",
            env!("CARGO_PKG_VERSION"),
            serde_json::to_string(transcripts.path()).unwrap()
        ),
    );
    write(".claude/rules/r.md", "# r\n");
    write("b/claude.md", "Owner: [file: no/such/b.rs:1].\n");
    commit(&ten_days_ago);
    // Read as a file's name, `**/CLAUDE.md` names neither change below, and
    // with its `*` stopping at a `/`, `docs/*.md` names not the last.
    write("a/CLAUDE.md", "Owner: [file: no/such/a.rs:1].\n");
    commit(&ten_days_ago);
    write("docs/guide/setup.md", "setup\n");
    let last = commit(&ten_days_ago);
    let session = transcripts.path().join("p/s.jsonl");
    std::fs::create_dir_all(session.parent().unwrap()).unwrap();
    std::fs::write(
        &session,
        format!(
            r#"{{"type":"user","uuid":"u","timestamp":"2026-10-01T09:00:00Z","sessionId":"s","version":"2.1.246","cwd":{},"origin":{{"kind":"human"}},"promptSource":"typed","message":{{"content":"go"}}}}"#,
            serde_json::to_string(&seen_root).unwrap()
        ) + "\n",
    )
    .unwrap();

    let answers = |variable: Option<&str>| {
        let run = |args: &[&str]| {
            let mut command = harness();
            command
                .args(args)
                .current_dir(root)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null");
            if let Some(variable) = variable {
                command.env(variable, "1");
            }
            let out = command.output().unwrap();
            serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
        };
        let classified = run(&[
            "lifecycle",
            "classify",
            "--kind",
            "rule",
            "--path",
            ".claude/rules/r.md",
            "--silence",
            "unmeasured",
        ]);
        let checked = run(&["check"]);
        let unresolved: Vec<PathBuf> = checked["data"]["findings"]
            .as_array()
            .unwrap_or_else(|| panic!("{checked}"))
            .iter()
            .filter(|f| f["slug"] == "evidence-claim-unresolved")
            .map(|f| {
                Path::new(f["location"]["path"].as_str().unwrap())
                    .strip_prefix(&seen_root)
                    .unwrap()
                    .to_path_buf()
            })
            .collect();
        let saved = run(&[
            "session",
            "baseline",
            "save",
            "--label",
            variable.unwrap_or("unset"),
            "--project",
            seen_root.to_str().unwrap(),
            "--since",
            "2026-01-01T00:00:00Z",
        ]);
        (
            classified["data"]["age_days"].clone(),
            unresolved,
            saved["data"]["harness"].clone(),
        )
    };

    let unset = answers(None);
    assert_eq!(
        unset,
        (
            serde_json::json!(10),
            vec![PathBuf::from("a/CLAUDE.md")],
            serde_json::json!({"head": last, "uncommitted": false}),
        )
    );
    for variable in [
        "GIT_LITERAL_PATHSPECS",
        "GIT_ICASE_PATHSPECS",
        "GIT_GLOB_PATHSPECS",
        "GIT_NOGLOB_PATHSPECS",
    ] {
        assert_eq!(answers(Some(variable)), unset, "{variable}=1");
    }
}
