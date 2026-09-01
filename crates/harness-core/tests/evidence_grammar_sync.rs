//! Drift guard for the claim grammar the shipped documentation teaches.
//!
//! Four documents tell an author how to mark a claim, and [`parse_claims`] is
//! what decides where a marker resolves. A form shown in one and dropped by
//! the other produces the worst outcome this gate has: the author writes the
//! citation the documentation asked for, `harnex check` reports clean, and
//! nothing was ever resolved.
//!
//! Both directions, because each catches a different mistake — an example the
//! parser no longer reads, and an anchor the parser gained that no document
//! teaches. [`shape`] is an exhaustive match, so a new anchor cannot be added
//! without arriving here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use harness_core::config::{EvidenceConfig, VerifierDecl};
use harness_core::evidence::{Anchor, ClaimKind, EvidenceVerifier, parse_claims};

const WHOLE: &str = "the whole file";
const LINE: &str = "a line";
const SECTION: &str = "a section";

/// Every anchor an author can write, and the documents that teach it.
///
/// Three documents spell the grammar and each spells it whole: `harness.toml`
/// beside the section that turns the check on, `rule-template.md` in the
/// skeleton every generated rule starts from, and README as the oracle's
/// public surface. Nothing else respells it — a fourth site is a fourth
/// chance to teach a form the parser does not read, and `SKILL.md` names the
/// concept and points at the template instead.
const GRAMMAR: &[(&str, &[&str])] = &[
    ("README.md", &[WHOLE, LINE, SECTION]),
    (
        "plugins/harnex/templates/common/harness.toml",
        &[WHOLE, LINE, SECTION],
    ),
    (
        "plugins/harnex/templates/common/rule-template.md",
        &[WHOLE, LINE, SECTION],
    ),
];

/// The anchor's shape, as the documentation names it.
fn shape(anchor: &Anchor) -> &'static str {
    match anchor {
        Anchor::Whole => WHOLE,
        Anchor::Line(_) => LINE,
        Anchor::Section(_) => SECTION,
    }
}

fn repo_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{relative} unreadable at {}: {e}", path.display()))
}

/// The anchors a document demonstrates, read the way the gate reads one.
///
/// A marker at a time, because these examples sit inside fences, HTML
/// comments and TOML comments — where the parser is right to ignore them,
/// and where this guard still has to know whether the form on the page is a
/// form it accepts. The leading whitespace goes with the container: kept, an
/// example indented under a bullet is an indented code block on its own, and
/// the guard would be measuring the layout rather than the grammar.
fn anchors_taught(text: &str) -> Vec<(String, Anchor)> {
    text.lines()
        .filter(|line| line.contains("[file:"))
        .flat_map(|line| parse_claims(line.trim_start()))
        .filter_map(|claim| match claim.kind {
            ClaimKind::File { path, anchor } => Some((path, anchor)),
            _ => None,
        })
        .collect()
}

#[test]
fn every_documented_form_is_one_the_parser_reads() {
    for (document, _) in GRAMMAR {
        let text = repo_file(document);
        let mut shown = 0;
        for line in text.lines().filter(|l| l.contains("[file:")) {
            shown += 1;
            assert!(
                !anchors_taught(line.trim_start()).is_empty(),
                "{document} shows a marker the parser resolves to nothing, so the form on \
                 the page is one the gate would drop in a rule: {line}"
            );
        }
        assert!(
            shown > 0,
            "{document} stopped showing the claim grammar at all"
        );
    }
}

#[test]
fn every_document_teaches_the_anchors_it_is_declared_to() {
    for (document, declared) in GRAMMAR {
        let taught: BTreeSet<&str> = anchors_taught(&repo_file(document))
            .iter()
            .map(|(_, anchor)| shape(anchor))
            .collect();
        let declared: BTreeSet<&str> = declared.iter().copied().collect();
        assert_eq!(
            taught, declared,
            "{document} teaches {taught:?} and is declared to teach {declared:?}"
        );
    }
}

#[test]
fn every_anchor_the_parser_reads_is_taught_somewhere() {
    // The denominator. An anchor added to the parser and to `shape` above,
    // and to no document, is a capability an author cannot discover — and one
    // whose absence from a rule looks exactly like a rule that needed no
    // citation.
    let taught: BTreeSet<&str> = GRAMMAR
        .iter()
        .flat_map(|(document, _)| anchors_taught(&repo_file(document)))
        .map(|(_, anchor)| shape(&anchor))
        .collect();
    let known: BTreeSet<&str> = [
        shape(&Anchor::Whole),
        shape(&Anchor::Line(1)),
        shape(&Anchor::Section(String::new())),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        taught, known,
        "the parser reads {known:?} and the shipped documentation teaches {taught:?}"
    );
}

#[test]
fn readmes_examples_resolve_through_the_gate_they_describe() {
    // README's examples name paths in this repository, so the grammar is not
    // only parsed but demonstrated — and the demonstration is run through
    // `EvidenceVerifier` rather than re-decided here. A second resolver in
    // the guard is how a documented anchor stops resolving in the gate while
    // the guard stays green (constitution IX).
    //
    // The other two documents ship to projects that do not exist yet, so
    // their examples are illustrative and only their parse is checked above.
    let verifier = EvidenceVerifier::new(&EvidenceConfig {
        default_provenance: "internal".into(),
        block_on_memory_only: false,
        verifiers: vec![VerifierDecl {
            provenance: "internal".into(),
            strategy: "file-path-line".into(),
            library_allowlist: Vec::new(),
            max_age_days: None,
        }],
        advisory_dir: "evidence".into(),
        advisories: Vec::new(),
    })
    .expect("the internal verifier is a declared strategy");

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let readme = repo_file("README.md");
    let shown: Vec<&str> = readme
        .lines()
        .filter(|line| line.contains("[file:"))
        .collect();
    assert_eq!(
        shown.len(),
        3,
        "README shows {} examples and this guard resolves each",
        shown.len()
    );
    for line in shown {
        let findings = verifier.verify_text(line.trim_start(), Path::new("README.md"), &root);
        assert!(
            findings.is_empty(),
            "README teaches an example the gate rejects: {line}\n{findings:#?}"
        );
    }
}
