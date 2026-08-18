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

/// Every pattern file is either validated by the oracle, or accounted for as a
/// destination no validator covers. Nothing is skipped silently.
///
/// harnex ships skills and a sub-agent now, not only rules, and a template that
/// is wrong is wrong in every project that installs it — the failure the whole
/// plugin exists to prevent, arriving from harnex itself.
///
/// The first version dispatched on three destination shapes and dropped
/// everything else through an unremarked `continue`. Eleven of twenty-five
/// files were checked and the docstring claimed all of them, so three content
/// defects shipped in exactly the files it was not looking at. The uncovered
/// set is now declared: a destination outside both lists fails here rather than
/// passing unexamined, which is the only part of this a test can guarantee.
///
/// Policy comes from the `harness.toml` the scaffold emits rather than a
/// literal, for the reason `.claude/rules/scaffold.md` gives — a restated policy
/// is one no real project has. That file turns `reject_unknown_keys` on for
/// three surfaces, so a stray frontmatter key here is a finding.
#[test]
fn every_pattern_surface_file_validates() {
    use harness_core::validate::{AgentValidator, RuleValidator, SkillValidator};

    let templates = patterns_dir().parent().unwrap().to_path_buf();
    let config = harness_core::config::Config::load_from(&templates.join("common/harness.toml"))
        .expect("the scaffolded harness.toml must load");
    let validate = config
        .validate
        .expect("scaffolded config declares validate");
    let rules = validate
        .rules
        .expect("scaffolded config declares validate.rules");
    let skills = validate
        .skills
        .expect("scaffolded config declares validate.skills");
    let agents = validate
        .agents
        .expect("scaffolded config declares validate.agents");

    let mut checked = 0usize;
    let mut unvalidated = 0usize;
    for pattern in &load_manifest().pattern {
        for file in &pattern.files {
            let src = patterns_dir().join(&pattern.slug).join(&file.template);
            let body = std::fs::read_to_string(&src).unwrap();
            let dest = std::path::Path::new(&file.destination);
            let landed = std::path::Path::new("/proj").join(dest);

            let findings = if file.destination.ends_with("/SKILL.md") {
                SkillValidator::new(&skills).validate_text(&body, &landed)
            } else if file.destination.starts_with(".claude/agents/") {
                AgentValidator::new(&agents).validate_text(&body, &landed)
            } else if file.destination.starts_with(".claude/rules/") {
                RuleValidator::new(&rules).validate_text(&body, &landed)
            } else {
                assert!(
                    UNVALIDATED_DESTINATIONS
                        .iter()
                        .any(|shape| matches_shape(&file.destination, shape)),
                    "pattern '{}' declares destination '{}', which no validator covers and \
                     UNVALIDATED_DESTINATIONS does not account for. Add a validator, or add the \
                     shape with the reason none exists — a silent skip is how three defects \
                     shipped in files this test was not looking at.",
                    pattern.slug,
                    file.destination
                );
                unvalidated += 1;
                continue;
            };
            checked += 1;
            assert!(
                findings.is_empty(),
                "pattern '{}' file '{}' would land at '{}' and fail the project's own \
                 validator: {findings:#?}",
                pattern.slug,
                file.template,
                file.destination
            );
        }
    }
    // Every declared file landed in exactly one of the two buckets, and both
    // are non-empty — a dispatch that stopped recognising a surface would move
    // its files into the uncovered bucket, and the assertion above would name
    // them rather than let the count quietly shrink.
    let declared: usize = load_manifest().pattern.iter().map(|p| p.files.len()).sum();
    assert_eq!(
        checked + unvalidated,
        declared,
        "every manifest file must be validated or accounted for"
    );
    assert!(checked > 0 && unvalidated > 0);
}

/// Destination shapes no oracle validator covers, each with the reason.
///
/// `{}` matches one path segment. This is the honest half of the test: harnex
/// ships these files and cannot mechanically check them, and saying so is what
/// keeps the gap from reading as coverage.
const UNVALIDATED_DESTINATIONS: &[&str] = &[
    // Skill resource files. The spec validates `SKILL.md` frontmatter; a
    // sibling procedure file the skill reads on demand has no declared shape.
    ".claude/skills/{}/{}.md",
    // Lens files. Their contract is stated in the review-lenses rule, which is
    // prose, so it is the review loop that reads them and not a validator.
    ".claude/lenses/{}.md",
    // Spec artifact templates — a project's own documents, not a Claude Code
    // surface.
    "specs/_template/{}.md",
    // A GitHub template and a hook script: neither is a Claude Code surface.
    ".github/pull_request_template.md",
    "hooks/{}.sh",
];

/// `{}` stands for exactly one path segment.
fn matches_shape(destination: &str, shape: &str) -> bool {
    let d: Vec<&str> = destination.split('/').collect();
    let s: Vec<&str> = shape.split('/').collect();
    if d.len() != s.len() {
        return false;
    }
    d.iter().zip(&s).all(|(dseg, sseg)| match sseg.find("{}") {
        None => dseg == sseg,
        Some(i) => {
            let (pre, post) = (&sseg[..i], &sseg[i + 2..]);
            dseg.len() >= pre.len() + post.len() && dseg.starts_with(pre) && dseg.ends_with(post)
        }
    })
}
