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

/// Every destination the manifest declares classifies into exactly one surface,
/// and every surface an oracle validator covers is validated.
///
/// harnex ships skills and a sub-agent now, not only rules, and a template that
/// is wrong is wrong in every project that installs it — the failure the whole
/// plugin exists to prevent, arriving from harnex itself.
///
/// One ordered classifier rather than a dispatch plus an excuse list. The first
/// attempt had both, and the excuse `.claude/skills/{}/{}.md` also matched
/// `.claude/skills/<name>/SKILL.md` — so breaking the dispatch moved the two
/// shipped skills into the excused bucket and every assertion still passed.
/// Two overlapping lists cannot express "everything else"; an ordered total
/// function can, and a destination it cannot classify fails here.
///
/// Policy comes from the `harness.toml` the scaffold emits rather than a
/// literal, for the reason `.claude/rules/scaffold.md` gives — a restated policy
/// is one no real project has. That file turns `reject_unknown_keys` on for
/// three surfaces, so a stray frontmatter key here is a finding.
#[derive(Debug, PartialEq, Eq)]
enum Surface {
    /// `SKILL.md` frontmatter — the spec declares its shape.
    Skill,
    /// A sub-agent definition.
    Agent,
    /// A path-scoped rule.
    Rule,
    /// A file a skill reads on demand. The spec declares no shape for one, so
    /// nothing can validate it — its correctness is the review's, not a gate's.
    SkillResource,
    /// A lens. Its contract is stated in the review-lenses rule, which is prose.
    Lens,
    /// A spec artifact template — the project's own document, not a Claude
    /// Code surface.
    SpecTemplate,
    /// A GitHub template or a hook script: neither is a Claude Code surface.
    OutsideClaudeCode,
}

impl Surface {
    /// Ordered and total. `SKILL.md` is tested before the skill-resource arm
    /// that would otherwise swallow it, which is the ordering the previous
    /// two-list version could not express.
    fn of(destination: &str) -> Option<Self> {
        let seg: Vec<&str> = destination.split('/').collect();
        Some(match seg.as_slice() {
            [".claude", "skills", _, "SKILL.md"] => Self::Skill,
            [".claude", "skills", _, f] if f.ends_with(".md") => Self::SkillResource,
            [".claude", "agents", f] if f.ends_with(".md") => Self::Agent,
            [".claude", "rules", f] if f.ends_with(".md") => Self::Rule,
            [".claude", "lenses", f] if f.ends_with(".md") => Self::Lens,
            ["specs", "_template", f] if f.ends_with(".md") => Self::SpecTemplate,
            [".github", "pull_request_template.md"] => Self::OutsideClaudeCode,
            ["hooks", f] if f.ends_with(".sh") => Self::OutsideClaudeCode,
            _ => return None,
        })
    }
}

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

    let mut seen: BTreeSet<&'static str> = BTreeSet::new();
    for pattern in &load_manifest().pattern {
        for file in &pattern.files {
            let surface = Surface::of(&file.destination).unwrap_or_else(|| {
                panic!(
                    "pattern '{}' declares destination '{}', which `Surface::of` cannot classify. \
                     Add its arm — either a validator surface, or one that says why none exists. \
                     A destination nothing classifies is how three defects shipped in files the \
                     earlier version of this test was not looking at.",
                    pattern.slug, file.destination
                )
            });
            let body =
                std::fs::read_to_string(patterns_dir().join(&pattern.slug).join(&file.template))
                    .unwrap();
            let landed = std::path::Path::new("/proj").join(&file.destination);

            let findings = match surface {
                Surface::Skill => {
                    seen.insert("skill");
                    SkillValidator::new(&skills).validate_text(&body, &landed)
                }
                Surface::Agent => {
                    seen.insert("agent");
                    AgentValidator::new(&agents).validate_text(&body, &landed)
                }
                Surface::Rule => {
                    seen.insert("rule");
                    RuleValidator::new(&rules).validate_text(&body, &landed)
                }
                // No oracle validator declares a shape for these. Saying so is
                // what keeps the gap from reading as coverage.
                Surface::SkillResource
                | Surface::Lens
                | Surface::SpecTemplate
                | Surface::OutsideClaudeCode => continue,
            };
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

    // Each validated surface was actually exercised. A classifier arm that
    // stopped matching would empty its bucket here rather than let the count
    // quietly shrink — the failure the excuse-list version could not see.
    assert_eq!(
        seen,
        ["agent", "rule", "skill"]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        "the pattern library must exercise every validator surface"
    );
}
