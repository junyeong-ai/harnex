//! harnex runs the harness it ships.
//!
//! Adopting a scaffold artifact into this repo makes it the same fact in two
//! places, and the manifest already says which artifacts have no project-owned
//! region: a `merge` fragment contributes one key to a shared JSON file, and a
//! `managed` artifact owns everything outside its sentinels, so neither is
//! comparable whole. Everything else is a byte copy and is held to its
//! template here (Constitution IX).
//!
//! Not hypothetical. The commit that adopted the foundation hooks left
//! `hooks/post-format.sh` six comment lines behind its template, and every
//! gate — full suite, clippy, `harness audit` — stayed green, because the
//! mechanism built to catch drift only reads artifacts marked `managed`.
//!
//! Which language's template a fixed destination came from is not knowable
//! here without detecting the stack, and detecting it to satisfy a test would
//! put project vocabulary in the assertion (Constitution VII). Matching *any*
//! shipped language's template is the honest question: drift makes a copy
//! match none of them.

use std::path::{Path, PathBuf};

use harness_core::policy::PermissionProfile;
use harness_core::scaffold::{Artifact, ScaffoldManifest};

/// Scaffold destinations this repo authored itself rather than adopting.
///
/// Adoption cannot be read off a file's existence: a destination the manifest
/// names is a place a project's own file may legitimately live. This repo
/// deliberately ships no `governance.md` or `artifact-lifecycle.md` today, and
/// on the day it writes its own, that is a note here — not a `managed` flag in
/// the shipped manifest, which would edit the product to describe a local
/// choice.
const AUTHORED: &[&str] = &[];

/// How many adopted artifacts this repo holds: the five foundation hooks plus
/// the language formatter.
const ADOPTED_ARTIFACT_COUNT: usize = 6;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn templates_root() -> PathBuf {
    repo_root().join("plugins/harnex/templates")
}

/// The languages an artifact could have been emitted for: every shipped one
/// when either half of the pair is parameterized, otherwise none at all.
fn candidate_languages(artifact: &Artifact) -> Vec<Option<&'static str>> {
    if artifact.template.contains("{lang}") || artifact.destination.contains("{lang}") {
        PermissionProfile::languages().map(Some).collect()
    } else {
        vec![None]
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn every_adopted_scaffold_artifact_matches_its_template() {
    let root = repo_root();
    let templates = templates_root();
    let manifest = ScaffoldManifest::load(&templates).expect("scaffold.toml loads");

    let mut compared = 0usize;
    for artifact in manifest.artifacts() {
        if artifact.merge.is_some() || artifact.managed {
            continue;
        }
        let langs = candidate_languages(artifact);
        let candidates: Vec<PathBuf> = langs
            .iter()
            .filter_map(|l| artifact.template_for(*l))
            .map(|t| templates.join(t))
            .filter(|p| p.exists())
            .collect();
        let mut destinations: Vec<PathBuf> = langs
            .iter()
            .filter_map(|l| artifact.destination_for(*l))
            .collect();
        destinations.sort();
        destinations.dedup();

        for destination in destinations {
            let adopted = root.join(&destination);
            if !adopted.exists() || AUTHORED.contains(&destination.to_string_lossy().as_ref()) {
                continue;
            }
            compared += 1;
            let body = read(&adopted);
            assert!(
                candidates.iter().any(|c| read(c) == body),
                "{} has drifted from every template that emits it.\n\
                 Templates checked: {:?}\n\
                 Either this repo has stopped shipping what it runs — re-copy \
                 the template — or this file is this repo's own work that \
                 happens to sit at a scaffold destination, in which case name \
                 it in AUTHORED. Do not reach for `managed` in scaffold.toml: \
                 that changes the shipped product to describe a local choice.",
                destination.display(),
                candidates
                    .iter()
                    .map(|c| c
                        .strip_prefix(&templates)
                        .unwrap_or(c)
                        .display()
                        .to_string())
                    .collect::<Vec<_>>(),
            );
        }
    }

    // An exact count, not a floor. `> 0` only catches total collapse, which is
    // the failure least likely to happen; deleting one adopted hook would sail
    // past it. Adopting or dropping an artifact is a decision, so it should
    // cost one deliberate edit here.
    assert_eq!(
        compared, ADOPTED_ARTIFACT_COUNT,
        "this repo now holds a different number of adopted scaffold artifacts; \
         if that was intended, update ADOPTED_ARTIFACT_COUNT"
    );
}
