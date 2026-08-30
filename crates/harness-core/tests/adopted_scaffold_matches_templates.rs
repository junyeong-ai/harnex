//! harnex runs the harness it ships.
//!
//! Adopting a scaffold artifact into this repo makes it the same fact in two
//! places, and the manifest already says how each artifact is held to its
//! template. `copy` is machinery whose template is the only statement of what
//! it does, so it is compared byte for byte. `merge` contributes a fragment to
//! a shared file, so it is compared by containment — the destination also
//! carries the other tier's fragment and this repo's own entries, and equality
//! would fail on all three. `managed` owns only what its sentinels bound and
//! `seed` is handed to the project outright, so neither is comparable here
//! (Constitution IX).
//!
//! Not hypothetical. The commit that adopted the foundation hooks left
//! `hooks/post-format.sh` six comment lines behind its template, and every
//! gate — full suite, clippy, `harnex audit` — stayed green, because the
//! mechanism built to catch drift only reads artifacts marked `managed`.
//!
//! Each destination is held to the template that emits IT, paired by the
//! language that produced both (`Artifact::resolved_pairs`). While the
//! formatter landed at one fixed `hooks/post-format.sh` the language was not
//! recoverable from the destination, and matching any shipped template was the
//! honest question; now that the destination carries it, the union would call
//! this repo undrifted while holding another language's formatter — ruff in a
//! Rust repo, which is the meta-failure the language matrix exists to prevent.

use std::path::{Path, PathBuf};

use harness_core::policy::PermissionProfile;
use harness_core::scaffold::{Artifact, Content, ScaffoldManifest};

/// Scaffold destinations this repo authored itself rather than adopting.
///
/// Adoption cannot be read off a file's existence: a destination the manifest
/// names is a place a project's own file may legitimately live. This repo
/// deliberately ships no `governance.md` or `artifact-lifecycle.md` today, and
/// on the day it writes its own, that is a note here — not a `managed` flag in
/// the shipped manifest, which would edit the product to describe a local
/// choice.
const AUTHORED: &[&str] = &[];

/// How many adopted `copy` artifacts this repo holds: the five foundation
/// hooks plus the language formatter.
const ADOPTED_ARTIFACT_COUNT: usize = 6;

/// The `merge` fragments this repo has adopted into `.claude/settings.json`.
///
/// Named rather than counted: the other languages' fragments resolve to the
/// same destination and are correctly absent from a Rust repo, so the question
/// is which fragments landed, not how many. A drifted rule leaves its template
/// here and the diff says which.
const ADOPTED_FRAGMENTS: &[&str] = &[
    "common/permissions.deny.json",
    "common/permissions.allow.json",
    "common/hooks.json",
    "rust/hooks.format.json",
    "rust/permissions.allow.json",
];

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
        if artifact.content != Content::Copy {
            continue;
        }
        for (template, destination) in artifact.resolved_pairs() {
            let adopted = root.join(&destination);
            if !adopted.exists() || AUTHORED.contains(&destination.to_string_lossy().as_ref()) {
                continue;
            }
            let source = templates.join(&template);
            if !source.exists() {
                continue;
            }
            compared += 1;
            assert_eq!(
                read(&adopted),
                read(&source),
                "{} has drifted from '{template}', the template that emits it.\n\
                 Either this repo has stopped shipping what it runs — re-copy \
                 the template — or this file is this repo's own work that \
                 happens to sit at a scaffold destination, in which case name \
                 it in AUTHORED. Do not reach for `managed` in scaffold.toml: \
                 that changes the shipped product to describe a local choice.",
                destination.display(),
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

#[test]
fn every_adopted_merge_fragment_is_present_in_its_destination() {
    // `.claude/settings.json` is this repo's own file AND the destination of
    // five shipped fragments. The byte comparison above cannot reach it, so
    // until now the deny floor could lose a rule — or the templates gain one —
    // with every gate green.
    let root = repo_root();
    let templates = templates_root();
    let manifest = ScaffoldManifest::load(&templates).expect("scaffold.toml loads");

    let mut landed: Vec<String> = Vec::new();
    for artifact in manifest.artifacts() {
        let Content::Merge { key } = &artifact.content else {
            continue;
        };
        for lang in candidate_languages(artifact) {
            let (Some(template), Some(destination)) =
                (artifact.template_for(lang), artifact.destination_for(lang))
            else {
                continue;
            };
            let (source, adopted) = (templates.join(&template), root.join(&destination));
            if !source.exists() || !adopted.exists() {
                continue;
            }
            let fragment: serde_json::Value = serde_json::from_str(&read(&source))
                .unwrap_or_else(|e| panic!("{} is not JSON: {e}", source.display()));
            let doc: serde_json::Value = serde_json::from_str(&read(&adopted))
                .unwrap_or_else(|e| panic!("{} is not JSON: {e}", adopted.display()));
            if harness_core::scaffold::fragment_landed(&doc, key, &fragment) {
                landed.push(template.clone());
            }
        }
    }
    landed.sort();
    landed.dedup();

    let mut expected: Vec<String> = ADOPTED_FRAGMENTS.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(
        landed, expected,
        "the fragments this repo carries have changed. A template missing from \
         the left side is one this repo stopped shipping what it runs — re-copy \
         it. One missing from the right is a fragment newly adopted, which is a \
         decision, so name it in ADOPTED_FRAGMENTS."
    );
}
