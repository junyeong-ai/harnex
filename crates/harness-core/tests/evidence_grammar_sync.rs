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
use std::path::PathBuf;

use harness_core::evidence::{Anchor, ClaimKind, parse_claims};

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
/// Line by line, because these examples sit inside fences, HTML comments and
/// TOML comments — where the parser is right to ignore them, and where this
/// guard still has to know whether the form on the page is a form it accepts.
fn anchors_taught(text: &str) -> Vec<(String, Anchor)> {
    text.lines()
        .filter(|line| line.contains("[file:"))
        .flat_map(|line| parse_claims(line).claims)
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
                !anchors_taught(line).is_empty(),
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
fn a_section_anchor_the_documentation_shows_resolves_in_this_repository() {
    // The examples name paths that exist here, so the grammar is not only
    // parsed but demonstrated against a real tree — a heading spelled in an
    // example and nowhere in the repository teaches a form the reader cannot
    // reproduce.
    let readme = repo_file("README.md");
    let cited: Vec<(String, Anchor)> = anchors_taught(&readme)
        .into_iter()
        .filter(|(_, anchor)| matches!(anchor, Anchor::Section(_)))
        .collect();
    assert!(!cited.is_empty(), "README shows no section anchor");
    for (path, anchor) in cited {
        let Anchor::Section(heading) = anchor else {
            unreachable!()
        };
        let target = repo_file(&path);
        let headings = target
            .lines()
            .filter(|line| {
                line.trim_start_matches(' ')
                    .trim_start_matches('#')
                    .trim()
                    .trim_end_matches('#')
                    .trim()
                    == heading
            })
            .count();
        assert_eq!(
            headings, 1,
            "README cites `{path} § {heading}`, which the file spells {headings} times"
        );
    }
}
