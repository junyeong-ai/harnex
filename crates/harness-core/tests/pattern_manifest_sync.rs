//! Drift guard for the harnex pattern library.
//!
//! `templates/patterns/manifest.toml` is the single source of truth for the
//! `extend pattern` verb: it lists every pattern, the files it installs, and
//! the concern areas the skill analyzes. This test verifies the manifest
//! agrees with the directories on disk — a pattern directory without a
//! manifest entry (or a manifest entry whose files are missing) fails the
//! build. Constitution IX: no hand-maintained fact in two places without a
//! guard.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    pattern: Vec<Pattern>,
}

#[derive(Debug, Deserialize)]
struct Pattern {
    slug: String,
    #[serde(default)]
    files: Vec<FileEntry>,
    #[serde(default)]
    analyze: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FileEntry {
    /// Source path relative to `templates/patterns/<slug>/`.
    template: String,
    /// Project-relative path the file is installed to.
    destination: String,
}

fn patterns_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/templates/patterns")
}

fn load_manifest() -> Manifest {
    let path = patterns_dir().join("manifest.toml");
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    toml::from_str(&raw).unwrap_or_else(|e| panic!("parse manifest.toml: {e}"))
}

/// Every directory under `templates/patterns/` is a manifest entry, and
/// every manifest entry has its directory.
#[test]
fn manifest_slugs_match_pattern_directories() {
    let manifest = load_manifest();
    let manifest_slugs: BTreeSet<String> =
        manifest.pattern.iter().map(|p| p.slug.clone()).collect();

    let mut dir_slugs = BTreeSet::new();
    for entry in std::fs::read_dir(patterns_dir()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            dir_slugs.insert(entry.file_name().to_string_lossy().to_string());
        }
    }

    assert_eq!(
        manifest_slugs, dir_slugs,
        "manifest.toml slugs drifted from templates/patterns/ directories"
    );
}

/// Every file a manifest entry declares actually exists on disk.
#[test]
fn manifest_declared_files_exist() {
    let manifest = load_manifest();
    for pattern in &manifest.pattern {
        let dir = patterns_dir().join(&pattern.slug);
        assert!(
            !pattern.files.is_empty(),
            "pattern '{}' declares no files",
            pattern.slug
        );
        for file in &pattern.files {
            let path = dir.join(&file.template);
            assert!(
                path.is_file(),
                "pattern '{}' declares '{}' but {} is missing",
                pattern.slug,
                file.template,
                path.display()
            );
        }
    }
}

/// Every install destination is project-relative and free of traversal — a
/// pattern must never write outside the target project.
#[test]
fn manifest_destinations_are_project_relative() {
    let manifest = load_manifest();
    for pattern in &manifest.pattern {
        for file in &pattern.files {
            let dest = std::path::Path::new(&file.destination);
            assert!(
                dest.is_relative()
                    && !file.destination.contains("..")
                    && !file.destination.is_empty(),
                "pattern '{}' destination '{}' must be a project-relative path without `..`",
                pattern.slug,
                file.destination
            );
        }
    }
}

/// Every pattern declares at least one analysis concern — the `extend
/// pattern` value proposition is analysis-driven customization, so a
/// pattern with no analyze step is a static copy and a design smell.
#[test]
fn every_pattern_declares_an_analysis_step() {
    let manifest = load_manifest();
    for pattern in &manifest.pattern {
        assert!(
            !pattern.analyze.is_empty(),
            "pattern '{}' has no analyze step — static copy, not project-fit",
            pattern.slug
        );
    }
}

/// No file on disk under a pattern directory is left undeclared (catches a
/// file added to a pattern dir but forgotten in the manifest).
#[test]
fn no_undeclared_files_in_pattern_directories() {
    let manifest = load_manifest();
    for pattern in &manifest.pattern {
        let dir = patterns_dir().join(&pattern.slug);
        let declared: BTreeSet<&str> = pattern.files.iter().map(|f| f.template.as_str()).collect();
        for path in walk_files(&dir) {
            let rel = path
                .strip_prefix(&dir)
                .unwrap()
                .to_string_lossy()
                .to_string();
            assert!(
                declared.contains(rel.as_str()),
                "pattern '{}' has undeclared file '{}' — add it to manifest.toml",
                pattern.slug,
                rel
            );
        }
    }
}

fn walk_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_files(&path));
        } else {
            out.push(path);
        }
    }
    out
}
