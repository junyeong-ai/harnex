//! Drift guard for the hook channel a guard speaks instead of the envelope.
//!
//! Constitution IX: the runtime owns the pair (`OPERATOR_CHANNEL_KEY`,
//! `SUPPRESS_OUTPUT_KEY`) — which of Claude Code's channels these commands
//! choose — and the two rule files recording that exception restate it.
//!
//! The list of restatements is itself the thing that goes stale, so it is not
//! only declared: every tracked file naming either key is classified here, and
//! one that is neither a restatement nor a named exception fails the build. A
//! guard whose subjects are enumerated by hand watches the ones somebody
//! remembered.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use harness_core::guard::{OPERATOR_CHANNEL_KEY, SUPPRESS_OUTPUT_KEY};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Files that restate which channel these commands write on.
const BODY_RESTATEMENTS: [&str; 1] = [".claude/rules/envelope.md"];
const CHANNEL_RESTATEMENTS: [&str; 2] = [".claude/rules/envelope.md", ".claude/rules/guard.md"];

/// The source that owns the choice, the site that writes it, and the case that
/// runs the command and reads the key back off its stdout.
const OWNER: [&str; 3] = [
    "crates/harness-core/src/guard/mod.rs",
    "crates/harness-cli/src/commands/guard.rs",
    "crates/harness-cli/tests/guard_stop_audit.rs",
];

/// Files naming the same field for a reason of their own. A rename here would
/// be this repository choosing a different channel for its own commands, and
/// none of these follow that choice.
const NOT_RESTATEMENTS: [(&str, &str); 5] = [
    (
        "plugins/harnex/reference/spec-facts.md",
        "Claude Code's vocabulary, measured from transcripts — the field is the runtime's to name",
    ),
    (
        "hooks/check-on-stop.sh",
        "the Stop advisory picks its own channel, and says in its own comment why",
    ),
    (
        "plugins/harnex/templates/common/check-on-stop.sh",
        "the template the line above is adopted from",
    ),
    (
        "plugins/harnex/templates/patterns/write-guard/write-guard.md",
        "a pattern telling a generated hook which channel to write on",
    ),
    (
        ".harness/harvest/2026-08-31-webloom.md",
        "a measurement of another project, true at the date it was taken",
    ),
];

/// Every tracked file naming either key, as git answers it: what ships is what
/// is tracked, and a scratch checkout beside the tree answers a directory walk.
fn files_naming_a_key() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for key in [OPERATOR_CHANNEL_KEY, SUPPRESS_OUTPUT_KEY] {
        let out = Command::new("git")
            .args(["grep", "-lF", key])
            .current_dir(root())
            .output()
            .expect("git grep runs");
        // `git grep` exits 1 on no match and 128 when it cannot answer at all,
        // and only the second is this test failing to run. Read as one, a
        // tree with no `.git` reports every file as classified.
        match out.status.code() {
            Some(0) => found.extend(
                String::from_utf8(out.stdout)
                    .expect("paths are UTF-8")
                    .lines()
                    .map(str::to_string),
            ),
            Some(1) => panic!("no tracked file names `{key}` — the source that owns it must"),
            _ => panic!(
                "git grep could not answer for `{key}`: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }
    found
}

#[test]
fn every_key_of_the_body_is_named_where_the_exception_is_recorded() {
    for rel in BODY_RESTATEMENTS {
        let content = read(rel);
        for key in [OPERATOR_CHANNEL_KEY, SUPPRESS_OUTPUT_KEY] {
            assert!(
                content.contains(key),
                "{rel} records the envelope exception without naming `{key}`"
            );
        }
    }
}

#[test]
fn the_operators_channel_is_named_wherever_the_outcome_is_explained() {
    for rel in CHANNEL_RESTATEMENTS {
        let content = read(rel);
        assert!(
            content.contains(OPERATOR_CHANNEL_KEY),
            "{rel} explains which reader a guard's outcome reaches without naming \
             `{OPERATOR_CHANNEL_KEY}`"
        );
    }
}

#[test]
fn every_file_naming_the_channel_is_classified() {
    let classified: BTreeSet<String> = BODY_RESTATEMENTS
        .iter()
        .chain(CHANNEL_RESTATEMENTS.iter())
        .chain(OWNER.iter())
        .map(|rel| (*rel).to_string())
        .chain(NOT_RESTATEMENTS.iter().map(|(rel, _)| (*rel).to_string()))
        .collect();
    let found = files_naming_a_key();

    let unclassified: Vec<&String> = found.difference(&classified).collect();
    assert!(
        unclassified.is_empty(),
        "these name a channel key and this guard says nothing about them — add each to a \
         restatement list, or to NOT_RESTATEMENTS with the reason it follows no rename: \
         {unclassified:?}"
    );
    let departed: Vec<&String> = classified.difference(&found).collect();
    assert!(
        departed.is_empty(),
        "these are classified and no longer name either key — a rename that left them behind, \
         or a classification outliving its file: {departed:?}"
    );
}

#[test]
fn the_classification_names_files_that_exist() {
    for (rel, _) in NOT_RESTATEMENTS {
        assert!(root().join(rel).is_file(), "{rel} is classified and absent");
    }
    for rel in OWNER.iter().chain(BODY_RESTATEMENTS.iter()) {
        assert!(Path::new(&root().join(rel)).is_file(), "{rel} is absent");
    }
}
