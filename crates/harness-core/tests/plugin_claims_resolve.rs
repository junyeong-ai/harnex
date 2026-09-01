//! The plugin's own marked claims resolve against the plugin.
//!
//! `reference/` is the knowledge the skill generates from, and a pointer in it
//! that resolves nowhere sends the skill to a section that does not exist —
//! which reads, from inside a generated harness, exactly like guidance that
//! was never written. The oracle's own `check` never opens these files: it
//! scans a project's `CLAUDE.md` and rules, and the plugin is neither. This is
//! that gate, pointed at the plugin.
//!
//! `templates/` is excluded, and the exclusion is the contract rather than an
//! omission: a template's claims are about the project it will be installed
//! into — `.claude/rules/…` paths that exist only after scaffolding — so they
//! are resolvable there and nowhere here.

use std::path::{Path, PathBuf};

use harness_core::config::{EvidenceConfig, VerifierDecl};
use harness_core::envelope::Finding;
use harness_core::evidence::EvidenceVerifier;

fn plugin_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex")
}

/// Every markdown document the plugin ships as its own, templates aside.
fn own_documents(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{} unreadable: {e}", dir.display()))
        .map(|e| e.expect("directory entry").path());
    for path in entries {
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "templates") {
                continue;
            }
            own_documents(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            out.push(path);
        }
    }
}

#[test]
fn every_claim_the_plugin_makes_about_itself_resolves() {
    let root = plugin_root();
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

    let mut documents = Vec::new();
    own_documents(&root, &mut documents);
    assert!(
        documents.len() > 5,
        "the plugin's own documents were not discovered: {documents:?}"
    );

    let findings: Vec<Finding> = documents
        .iter()
        .flat_map(|path| {
            verifier
                .verify_file(path, &root)
                .unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()))
        })
        .collect();

    assert!(
        findings.is_empty(),
        "the plugin cites what it does not carry:\n{}",
        findings
            .iter()
            .map(|f| format!("  {:?} — {}", f.location, f.message))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn a_pointer_into_a_renamed_section_is_what_this_guard_catches() {
    // The guard proves nothing unless the plugin's documents actually carry
    // section anchors — a corpus of file-only claims would pass while every
    // heading in the skill was renamed.
    let root = plugin_root();
    let mut documents = Vec::new();
    own_documents(&root, &mut documents);

    let sections: Vec<String> = documents
        .iter()
        .flat_map(|path| {
            let text = std::fs::read_to_string(path).expect("readable");
            harness_core::evidence::parse_claims(&text)
                .claims
                .into_iter()
                .filter_map(|claim| match claim.kind {
                    harness_core::evidence::ClaimKind::File {
                        path,
                        anchor: harness_core::evidence::Anchor::Section(heading),
                    } => Some(format!("{path} § {heading}")),
                    _ => None,
                })
        })
        .collect();

    assert!(
        !sections.is_empty(),
        "no section anchor in the plugin's own documents, so this guard would pass \
         over any heading rename"
    );
}
