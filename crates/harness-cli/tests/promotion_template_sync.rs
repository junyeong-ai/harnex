//! The promotion loop's two shipped ends meet at the released binary.
//!
//! A spec's wrapup EMITS an observation and the curate pass DRAINS it. Neither
//! half is code — one is a template a project fills in, the other a skill a
//! project runs — and nothing but this test holds either to the CLI surface
//! between them (constitution IX). Broken, the loop goes silent in both
//! directions: an emit whose flags no longer parse writes nothing, and a drain
//! written against a field the envelope stopped carrying reads the absence as
//! an answer.
//!
//! So the emit is not described here and then approximated — it is taken out
//! of the template, filled in, and run. A guard that checks tokens and then
//! executes its own correct copy of the command reports coverage it does not
//! have, which is the failure this repository refuses everywhere else.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const WRAPUP: &str = "templates/patterns/spec-workflow/specs/wrapup.md";
const CURATE: &str = "templates/common/skills/harness-curate/SKILL.md";
const SPEC_SKILL: &str = "templates/patterns/spec-workflow/skill/SKILL.md";
const SCAFFOLD_CONFIG: &str = "templates/common/harness.toml";

/// The flags the emit owes, as a set: three distinct options, so a template
/// that repeats one or drops one fails on the set rather than on a count.
const EMIT_FLAGS: [&str; 3] = ["tag", "text", "source"];

/// The survey fields the curate skill reads by name. Both directions are
/// checked against this list: a field the envelope drops while the prose still
/// names it, and one the prose starts naming unwatched.
const DRAIN_READS: &[&str] = &[
    "observations_read",
    "decisions_read",
    "groups_considered",
    "groups_resolved",
    "instance_count",
    "sources",
];

fn template(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{relative} unreadable at {}: {e}", path.display()))
}

/// A project carrying the config the scaffold ships, and nothing observed yet.
fn scaffolded() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("temp project");
    std::fs::write(
        project.path().join("harness.toml"),
        template(SCAFFOLD_CONFIG),
    )
    .expect("write the scaffold config");
    project
}

/// One envelope of the built binary, run inside `dir`.
fn envelope(dir: &Path, args: &[String]) -> serde_json::Value {
    let spelled = args.join(" ");
    let out = Command::new(env!("CARGO_BIN_EXE_harnex"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("`harnex {spelled}` did not run: {e}"));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "`harnex {spelled}` emitted no envelope ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

fn data(dir: &Path, args: &[&str]) -> serde_json::Value {
    let owned: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    let envelope = envelope(dir, &owned);
    assert_eq!(
        envelope["ok"],
        serde_json::Value::Bool(true),
        "`harnex {}` failed: {envelope}",
        args.join(" ")
    );
    envelope["data"].clone()
}

/// The emit the wrapup instructs, as an argv, with each `<placeholder>`
/// replaced by `fill`.
///
/// Splitting honours the double quotes the template needs around its text
/// argument; a shell would, and the instruction is written to be pasted into
/// one.
fn instructed_emit(fill: &dyn Fn(&str) -> String) -> Vec<String> {
    let wrapup = template(WRAPUP);
    let line = wrapup
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("harnex lifecycle observe"))
        .expect("wrapup.md instructs the emit as a runnable command line");

    let mut argv = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut started = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    argv.push(std::mem::take(&mut token));
                    started = false;
                }
            }
            c => {
                token.push(c);
                started = true;
            }
        }
    }
    if started {
        argv.push(token);
    }
    assert!(!quoted, "the emit line leaves a quote open: {line}");
    assert_eq!(
        argv.first().map(String::as_str),
        Some("harnex"),
        "the emit line must be the command itself"
    );

    argv.into_iter()
        .skip(1)
        .map(
            |arg| match arg.strip_prefix('<').and_then(|a| a.strip_suffix('>')) {
                Some(name) => fill(name),
                None => arg,
            },
        )
        .collect()
}

/// Every field a document cites — a code span holding one identifier and
/// nothing else.
///
/// Two things are excluded, and both are commands rather than citations.
/// Fenced blocks hold the invocations the skill runs, and an inline span can
/// hold one too: `harnex lifecycle candidates` names a subcommand, not the
/// survey's `candidates` field, so reading its words as citations would report
/// a field consumption the document never wrote. A field is named on its own.
fn cited_identifiers(doc: &str, body: &str) -> BTreeSet<String> {
    let unfenced: String = body.split("```").step_by(2).collect::<Vec<_>>().join("\n");
    let spans: Vec<&str> = unfenced.split('`').collect();
    assert!(
        spans.len() % 2 == 1,
        "{doc} leaves a ` unclosed outside its fences, so this guard cannot \
         tell a citation from prose"
    );
    spans
        .iter()
        .skip(1)
        .step_by(2)
        .map(|span| span.trim())
        .filter(|span| {
            !span.is_empty() && span.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        .map(str::to_string)
        .collect()
}

#[test]
fn the_observation_the_wrapup_instructs_lands_where_the_curate_pass_reads_it() {
    let flags: BTreeSet<String> = instructed_emit(&|name| format!("<{name}>"))
        .iter()
        .filter_map(|a| a.strip_prefix("--").map(str::to_string))
        .collect();
    let owed: BTreeSet<String> = EMIT_FLAGS.iter().map(|f| (*f).to_string()).collect();
    assert_eq!(
        flags, owed,
        "the wrapup instructs flags {flags:?} and the emit owes {owed:?} — a \
         repeated or missing option writes every spec's harness proposals to nothing"
    );

    // The claim no token check can make: the instructed command, filled in and
    // run against the config the scaffold ships, moves the numbers the drain
    // reads.
    let project = scaffolded();
    let before = data(project.path(), &["lifecycle", "candidates"]);
    assert_eq!(
        before["observations_read"], 0,
        "a fresh scaffold has observed nothing until a wrapup does"
    );

    for slug in ["spec-a", "spec-b"] {
        let argv = instructed_emit(&|name| match name {
            "slug" => slug.to_string(),
            "topic" => "naming".to_string(),
            _ => "the same constraint, in the standing wording".to_string(),
        });
        let envelope = envelope(project.path(), &argv);
        assert_eq!(
            envelope["ok"],
            serde_json::Value::Bool(true),
            "the command the wrapup instructs must run: {envelope}"
        );
    }

    let after = data(project.path(), &["lifecycle", "candidates"]);
    assert_eq!(
        after["observations_read"], 2,
        "the emit must land in the ledger the drain reads"
    );
    assert_eq!(
        after["groups_considered"], 1,
        "one wording is one group, however many specs saw it"
    );
}

#[test]
fn the_spec_skill_may_run_the_emit_its_wrapup_instructs() {
    // A skill's `allowed-tools` is the whole tool surface it runs under, so an
    // instruction naming a command the frontmatter does not grant is an
    // instruction that prompts or stops — which is how the emit half came to
    // never fire.
    let verb = instructed_emit(&|name| format!("<{name}>"))
        .iter()
        .take_while(|arg| !arg.starts_with("--"))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let grant = format!("Bash(harnex {verb} *)");
    let skill = template(SPEC_SKILL);
    let allowed = skill
        .lines()
        .find_map(|l| l.strip_prefix("allowed-tools:"))
        .expect("the spec skill declares allowed-tools");
    assert!(
        allowed.contains(&grant),
        "the wrapup instructs `harnex {verb}` and the spec skill grants {allowed:?} — \
         it must carry {grant}"
    );
}

#[test]
fn the_drain_prose_is_written_against_fields_the_survey_carries() {
    let project = scaffolded();
    let survey = data(project.path(), &["lifecycle", "candidates"]);
    let mut carried: BTreeSet<String> = survey
        .as_object()
        .expect("the survey answers with an object")
        .keys()
        .cloned()
        .collect();
    // A candidate's own fields are part of what the drain reads, and an empty
    // ledger carries no candidate to read them off.
    carried.extend(
        serde_json::to_value(harness_core::lifecycle::PromotionCandidate {
            tag: String::new(),
            normalized_text: String::new(),
            instance_count: 0,
            span_days: 0,
            first_seen: jiff::Timestamp::UNIX_EPOCH,
            last_seen: jiff::Timestamp::UNIX_EPOCH,
            sources: Vec::new(),
        })
        .expect("a candidate serialises")
        .as_object()
        .expect("into an object")
        .keys()
        .cloned(),
    );

    let cited = cited_identifiers(CURATE, &template(CURATE));
    let read: BTreeSet<String> = cited.intersection(&carried).cloned().collect();
    let watched: BTreeSet<String> = DRAIN_READS.iter().map(|f| (*f).to_string()).collect();

    assert_eq!(
        read, watched,
        "the curate skill reads {read:?} out of the survey and this guard watches \
         {watched:?}. Register the new citation in DRAIN_READS, or restore the field \
         the prose still names"
    );
}
