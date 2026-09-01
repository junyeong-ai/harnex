//! The promotion loop's two shipped ends meet at the released binary.
//!
//! A spec's wrapup EMITS an observation and the curate pass DRAINS it. Neither
//! half is code — one is a template a project fills in, the other a skill a
//! project runs — and nothing but this test holds either to the CLI surface
//! between them (constitution IX). Broken, the loop goes silent in both
//! directions: an emit whose flags no longer parse writes nothing, and a drain
//! written against a field the envelope stopped carrying reads the absence as
//! an answer.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const WRAPUP: &str = "templates/patterns/spec-workflow/specs/wrapup.md";
const CURATE: &str = "templates/common/skills/harness-curate/SKILL.md";
const SCAFFOLD_CONFIG: &str = "templates/common/harness.toml";

/// The fields the curate skill reads out of the survey. Both directions are
/// checked against this list: a field the envelope drops while the prose still
/// names it, and one the prose starts naming unwatched.
const DRAIN_READS: &[&str] = &[
    "candidates",
    "observations_read",
    "groups_considered",
    "groups_resolved",
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

/// `data` from one envelope of the built binary, run inside `dir`.
fn data(dir: &Path, args: &[&str]) -> serde_json::Value {
    let spelled = args.join(" ");
    let out = Command::new(env!("CARGO_BIN_EXE_harnex"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("`harnex {spelled}` did not run: {e}"));
    let envelope: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "`harnex {spelled}` emitted no envelope ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    });
    assert_eq!(
        envelope["ok"],
        serde_json::Value::Bool(true),
        "`harnex {spelled}` failed: {envelope}"
    );
    envelope["data"].clone()
}

fn help(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_harnex"))
        .args(args)
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "`{args:?} --help` must exist");
    String::from_utf8(out.stdout).unwrap()
}

/// Every identifier a document names inside a code span.
///
/// A citation is a code span, not a word: `candidates` and `groups` are also
/// ordinary English in these documents, and searching the prose for them
/// passes whatever the prose says.
fn cited_identifiers(doc: &str, body: &str) -> BTreeSet<String> {
    let spans: Vec<&str> = body.split('`').collect();
    assert!(
        spans.len() % 2 == 1,
        "{doc} leaves a ` unclosed, so this guard cannot tell a citation from prose"
    );
    spans
        .iter()
        .skip(1)
        .step_by(2)
        .flat_map(|span| span.split(|c: char| !c.is_alphanumeric() && c != '_'))
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn the_observation_the_wrapup_instructs_lands_where_the_curate_pass_reads_it() {
    let wrapup = template(WRAPUP);
    let emit = wrapup
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("harnex lifecycle observe"))
        .expect("wrapup.md instructs the emit as a runnable command line");

    let surface = help(&["lifecycle", "observe"]);
    let mut flags = 0;
    for flag in emit.split_whitespace().filter_map(|t| t.strip_prefix("--")) {
        assert!(
            surface.contains(&format!("--{flag}")),
            "wrapup.md instructs --{flag}, which `lifecycle observe --help` does not list — \
             every spec's harness proposals would be written to nothing"
        );
        flags += 1;
    }
    assert_eq!(flags, 3, "the emit names its tag, its text and its source");

    // The claim the string check above cannot make: running that command
    // against the config the scaffold ships moves the numbers the drain reads.
    let project = scaffolded();
    let before = data(project.path(), &["lifecycle", "candidates"]);
    assert_eq!(
        before["observations_read"], 0,
        "a fresh scaffold has observed nothing until a wrapup does"
    );

    for slug in ["spec-a", "spec-b"] {
        data(
            project.path(),
            &[
                "lifecycle",
                "observe",
                "--tag",
                "naming",
                "--text",
                "the same constraint, in the standing wording",
                "--source",
                slug,
            ],
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
fn the_drain_prose_is_written_against_fields_the_survey_carries() {
    let project = scaffolded();
    let carried: BTreeSet<String> = data(project.path(), &["lifecycle", "candidates"])
        .as_object()
        .expect("the survey answers with an object")
        .keys()
        .cloned()
        .collect();

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
