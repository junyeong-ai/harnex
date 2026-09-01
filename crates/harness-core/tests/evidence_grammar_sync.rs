//! Drift guard for the claim grammar the shipped documentation teaches.
//!
//! Three documents tell an author how to mark a claim, and [`parse_claims`] is
//! what decides where a marker resolves. A form shown in one and dropped by
//! the other produces the worst outcome this gate has: the author writes the
//! citation the documentation asked for, `harnex check` reports clean, and
//! nothing was ever resolved.
//!
//! Both directions, because each catches a different mistake — an example the
//! parser no longer reads, and an anchor the parser gained that no document
//! teaches. The denominator is `AnchorKind::ALL`, which the macro generates:
//! an anchor added to `Anchor` forces an arm in `Anchor::kind`, which forces a
//! variant here, and no step in that chain is a list anyone keeps by hand.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use harness_core::config::{EvidenceConfig, VerifierDecl};
use harness_core::evidence::{AnchorKind, ClaimKind, EvidenceVerifier, parse_claims};

/// Every anchor an author can write, and the documents that teach it.
///
/// Three documents spell the grammar and each spells it whole: `harness.toml`
/// beside the section that turns the check on, `rule-template.md` in the
/// skeleton every generated rule starts from, and README as the oracle's
/// public surface. Nothing else respells it — a fourth site is a fourth
/// chance to teach a form the parser does not read, and `SKILL.md` names the
/// concept and points at the template instead.
const GRAMMAR: &[(&str, &[AnchorKind])] = &[
    ("README.md", &[AnchorKind::Whole, AnchorKind::Line, AnchorKind::Section]),
    (
        "plugins/harnex/templates/common/harness.toml",
        &[AnchorKind::Whole, AnchorKind::Line, AnchorKind::Section],
    ),
    (
        "plugins/harnex/templates/common/rule-template.md",
        &[AnchorKind::Whole, AnchorKind::Line, AnchorKind::Section],
    ),
];

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
fn anchors_taught(text: &str) -> Vec<(String, AnchorKind)> {
    text.lines()
        .filter(|line| line.contains("[file:"))
        .flat_map(|line| parse_claims(line.trim_start()))
        .filter_map(|claim| match claim.kind {
            ClaimKind::File { path, anchor } => Some((path, anchor.kind())),
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
        let taught: BTreeSet<AnchorKind> = anchors_taught(&repo_file(document))
            .iter()
            .map(|(_, kind)| *kind)
            .collect();
        let declared: BTreeSet<AnchorKind> = declared.iter().copied().collect();
        assert_eq!(
            taught, declared,
            "{document} teaches {taught:?} and is declared to teach {declared:?}"
        );
    }
}

#[test]
fn every_anchor_the_parser_reads_is_taught_somewhere() {
    // The denominator. An anchor the parser reads and no document teaches is
    // a capability an author cannot discover — and one whose absence from a
    // rule looks exactly like a rule that needed no citation.
    let taught: BTreeSet<AnchorKind> = GRAMMAR
        .iter()
        .flat_map(|(document, _)| anchors_taught(&repo_file(document)))
        .map(|(_, kind)| kind)
        .collect();
    let known: BTreeSet<AnchorKind> = AnchorKind::ALL.iter().copied().collect();
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

/// Files that carry the marker and are checked by nothing here, each with
/// the reason it is allowed to.
const ALLOWED_UNCHECKED: &[(&str, &str)] = &[(
    "crates/harness-core/CLAUDE.md",
    "a crate-scoped CLAUDE.md, which `harnex check` does not scan — its candidate set is the \
     root CLAUDE.md and the rule glob; widening it is a gate change for every installed harness",
)];

#[test]
fn every_tracked_file_carrying_the_marker_is_accounted_for() {
    // The trigger is the corpus, not a list. A document that starts spelling
    // the grammar — teaching it, or making a claim — is caught here the moment
    // it is tracked, and its author either routes it to the gate that reads
    // it, adds it to `GRAMMAR`, or names it above with a reason. A
    // hand-written whitelist can only catch what it already knows about.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let listing = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(&root)
        .output()
        .expect("git ls-files runs in this repository");
    assert!(listing.status.success(), "git ls-files failed: {listing:?}");
    let tracked = String::from_utf8(listing.stdout).expect("tracked paths are UTF-8");

    let carrying: Vec<&str> = tracked
        .split('\0')
        .filter(|path| !path.is_empty())
        .filter(|path| {
            std::fs::read(root.join(path))
                .map(|bytes| bytes.windows(6).any(|w| w == b"[file:"))
                .unwrap_or(false)
        })
        .collect();
    assert!(carrying.len() > 5, "the marker was found almost nowhere: {carrying:?}");

    let accounted = |path: &str| {
        // Resolved by `harnex check` over this repository.
        (path == "CLAUDE.md" || (path.starts_with(".claude/rules/") && path.ends_with(".md")))
            // Resolved by `plugin_claims_resolve`.
            || (path.starts_with("plugins/harnex/")
                && path.ends_with(".md")
                && !path.starts_with("plugins/harnex/templates/"))
            // Parsed by this guard.
            || GRAMMAR.iter().any(|(document, _)| *document == path)
            // Source and tests: the marker is code.
            || (path.starts_with("crates/") && path.ends_with(".rs"))
            // Ledgers: the marker is data.
            || path.starts_with(".harness/")
            || ALLOWED_UNCHECKED.iter().any(|(allowed, _)| *allowed == path)
    };
    let stray: Vec<&str> = carrying
        .iter()
        .copied()
        .filter(|path| !accounted(path))
        .collect();
    assert!(
        stray.is_empty(),
        "these tracked files carry the claim marker and nothing checks them — route each to \
         the gate that reads it, add it to `GRAMMAR`, or name it in `ALLOWED_UNCHECKED` with \
         a reason:\n  {}",
        stray.join("\n  ")
    );
    for (allowed, reason) in ALLOWED_UNCHECKED {
        assert!(
            carrying.contains(allowed),
            "`{allowed}` is allowed unchecked ({reason}) but no longer carries the marker — \
             drop the allowance"
        );
    }
}
