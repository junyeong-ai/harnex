//! A pattern's marked claims resolve in the project it installs into.
//!
//! The templates under `templates/patterns/` cite the artifacts their own
//! pattern installs — a skill pointing a sub-agent at a section of a rule —
//! and those paths exist only after scaffolding. Here `manifest.toml` says
//! where each file lands, so the layout is built in a temporary project and
//! the shipped verifier runs over it: the gate a generated harness runs, on
//! the harness before it is generated. A pointer into a section the pattern
//! renamed fails here rather than in the first project that installs it.
//!
//! Every pattern is installed into one tree, so a pattern may cite another
//! pattern's artifact; a template citing something only `scaffold.toml`'s
//! common tier installs would fail here, and the honest fix is to widen the
//! tree, not the exclusion.

use std::path::PathBuf;

use harness_core::config::{EvidenceConfig, VerifierDecl};
use harness_core::evidence::EvidenceVerifier;
use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    pattern: Vec<Pattern>,
}

#[derive(Deserialize)]
struct Pattern {
    #[serde(default)]
    files: Vec<FileEntry>,
}

#[derive(Deserialize)]
struct FileEntry {
    template: String,
    destination: String,
}

fn patterns_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/templates/patterns")
}

#[test]
fn every_pattern_claim_resolves_in_the_layout_it_installs_to() {
    let raw = std::fs::read_to_string(patterns_dir().join("manifest.toml")).unwrap();
    let manifest: Manifest = toml::from_str(&raw).unwrap();
    let project = tempfile::tempdir().unwrap();

    let mut installed = Vec::new();
    for (slug_dir, pattern) in std::fs::read_dir(patterns_dir())
        .unwrap()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path())
        .zip(std::iter::repeat(()))
        .map(|(dir, ())| dir)
        .flat_map(|dir| manifest.pattern.iter().map(move |p| (dir.clone(), p)))
        .filter(|(dir, p)| {
            p.files
                .iter()
                .all(|f| dir.join(&f.template).is_file())
                && !p.files.is_empty()
        })
    {
        for file in &pattern.files {
            let source = slug_dir.join(&file.template);
            let destination = project.path().join(&file.destination);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::copy(&source, &destination).unwrap();
            installed.push(destination);
        }
    }
    installed.sort();
    installed.dedup();
    assert!(installed.len() > 10, "the patterns installed almost nothing: {installed:?}");

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
    .unwrap();

    let mut claims = 0;
    for path in installed.iter().filter(|p| p.extension().is_some_and(|e| e == "md")) {
        let text = std::fs::read_to_string(path).unwrap();
        claims += harness_core::evidence::parse_claims(&text).len();
        let findings = verifier.verify_text(&text, path, project.path());
        assert!(
            findings.is_empty(),
            "{} cites what its pattern does not install:\n{findings:#?}",
            path.strip_prefix(project.path()).unwrap().display()
        );
    }
    assert!(claims > 0, "no pattern carries a claim, so this guard resolves nothing");
}
