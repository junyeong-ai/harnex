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
//! One project per pattern, because `extend pattern <slug>` installs one at a
//! time: a claim that resolves only because a second pattern happens to be
//! present would pass here and fail in the first project to install the
//! pattern alone. A template citing something only `scaffold.toml`'s common
//! tier installs fails here too, and the honest fix is to widen the tree.

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
    slug: String,
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

fn internal_verifier() -> EvidenceVerifier {
    EvidenceVerifier::new(&EvidenceConfig {
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
    .unwrap()
}

#[test]
fn every_pattern_claim_resolves_in_the_layout_it_installs_to_alone() {
    let raw = std::fs::read_to_string(patterns_dir().join("manifest.toml")).unwrap();
    let manifest: Manifest = toml::from_str(&raw).unwrap();
    let verifier = internal_verifier();
    let mut claims = 0;
    let mut patterns = 0;

    for pattern in &manifest.pattern {
        let dir = patterns_dir().join(&pattern.slug);
        assert!(dir.is_dir(), "manifest names `{}` and no directory carries it", pattern.slug);
        let project = tempfile::tempdir().unwrap();
        let mut installed = Vec::new();
        for file in &pattern.files {
            assert!(
                !file.destination.contains('{'),
                "`{}` installs to a parameterised destination this test does not resolve: {}",
                pattern.slug,
                file.destination
            );
            let destination = project.path().join(&file.destination);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::copy(dir.join(&file.template), &destination).unwrap();
            installed.push(destination);
        }
        patterns += 1;
        for path in installed.iter().filter(|p| p.extension().is_some_and(|e| e == "md")) {
            let text = std::fs::read_to_string(path).unwrap();
            claims += harness_core::evidence::parse_claims(&text).len();
            let findings = verifier.verify_text(&text, path, project.path());
            assert!(
                findings.is_empty(),
                "`{}` installed alone: {} cites what the pattern does not install:\n{findings:#?}",
                pattern.slug,
                path.strip_prefix(project.path()).unwrap().display()
            );
        }
    }
    assert!(patterns > 3, "the manifest declares almost no patterns");
    assert!(claims > 0, "no pattern carries a claim, so this guard resolves nothing");
}
