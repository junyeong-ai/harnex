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
                    && !file
                        .destination
                        .split('/')
                        .any(|seg| seg == "." || seg == ".." || seg.is_empty())
                    && !file.destination.is_empty(),
                "pattern '{}' destination '{}' must be project-relative with no `.`, `..` \
                 or empty segment. `Path::file_name` normalizes a lone `.`, so the shipped \
                 SkillValidator and this file's own path splitters would disagree about \
                 which directory the file lands in.",
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
    /// An output style. The oracle validates one; the classifier had no arm for
    /// it, so a pattern shipping one was rejected as unclassifiable.
    OutputStyle,
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
    /// Ordered and total. The validated arms ask each validator's own `GLOB`
    /// rather than restating it: the classifier was a third hand-written copy
    /// of "which validator covers which path", and it had already drifted from
    /// two of them — `RuleValidator` and `AgentValidator` discover recursively
    /// (`**/*.md`) where this matched one level, and `OutputStyleValidator`
    /// had no arm at all, so a pattern shipping an output style the oracle can
    /// validate was rejected as unclassifiable.
    ///
    /// `SKILL.md` is still tested before the skill-resource arm that would
    /// otherwise swallow it — the skill glob is `*/SKILL.md`, so a sibling
    /// resource matches no validator and needs an arm of its own.
    fn of(destination: &str) -> Option<Self> {
        use harness_core::validate::{
            AgentValidator, OutputStyleValidator, RuleValidator, SkillValidator, SurfaceValidator,
        };
        fn covers<'p, V: SurfaceValidator<'p>>(destination: &str) -> bool {
            glob::Pattern::new(V::GLOB).is_ok_and(|p| p.matches(destination))
        }
        if covers::<SkillValidator>(destination) {
            return Some(Self::Skill);
        }
        if covers::<AgentValidator>(destination) {
            return Some(Self::Agent);
        }
        if covers::<RuleValidator>(destination) {
            return Some(Self::Rule);
        }
        if covers::<OutputStyleValidator>(destination) {
            return Some(Self::OutputStyle);
        }
        let seg: Vec<&str> = destination.split('/').collect();
        Some(match seg.as_slice() {
            [".claude", "skills", _, f] if f.ends_with(".md") => Self::SkillResource,
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
    use harness_core::validate::{
        AgentValidator, OutputStyleValidator, RuleValidator, SkillValidator,
    };

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
    let output_styles = validate
        .output_styles
        .expect("scaffolded config declares validate.output_styles");

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
                Surface::OutputStyle => {
                    seen.insert("output-style");
                    OutputStyleValidator::new(&output_styles).validate_text(&body, &landed)
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

    // The library exercises at least one validator, and everything it exercised
    // is a surface an oracle validator covers.
    //
    // Not an equality against a fixed set: that hardcodes the surfaces the
    // library happens to ship today, and adding a legitimate one — an output
    // style, which the oracle validates — failed the test for shipping it.
    //
    // A classifier arm that stops matching is caught without this. A rule,
    // agent or output-style destination falling out of its arm classifies as
    // nothing and panics above; a `SKILL.md` falling out of its arm lands in
    // `SkillResource`, where `every_skill_directory_has_its_entry_point_at_install_time`
    // counts zero entry points and fails. This assertion is the floor, not the
    // guard.
    assert!(
        !seen.is_empty(),
        "the pattern library must exercise at least one validator surface"
    );
    let known: BTreeSet<&str> = ["agent", "output-style", "rule", "skill"]
        .into_iter()
        .collect();
    assert!(
        seen.is_subset(&known),
        "exercised an unknown surface: {:?}",
        seen.difference(&known).collect::<Vec<_>>()
    );
}

/// Every skill directory a pattern installs into has its entry point available
/// when that pattern installs — from the pattern itself, or from the scaffold.
///
/// Presence-per-surface is not coverage-per-file. With two skills shipped, one
/// escaping validation leaves the other in `seen` and the set assertion notices
/// nothing — reproduced two ways: a one-character typo in the manifest
/// (`Skill.md`), and a classifier arm narrowed to one skill name. Both left
/// green tests while a skill shipped that Claude Code does not load as one.
///
/// Grouped per pattern, because a pattern is the install unit: `extend pattern
/// <slug>` installs one and nothing else. Grouping across all of them accepted
/// a pattern shipping a file into another pattern's skill directory, which
/// composes fine when both are installed and leaves a resource belonging to no
/// skill when that one is installed alone. The foundation tier is the exception
/// and not a hole in the rule: the scaffold emits it before any pattern runs, so
/// its skill directory is present by the time a pattern could extend it.
#[test]
fn every_skill_directory_has_its_entry_point_at_install_time() {
    // Through the crate's own loader, not a second parser. `scaffold.toml` is a
    // closed schema and `ScaffoldManifest::load` is what enforces that; an
    // ad-hoc struct here would be a second representation of the same shape —
    // and the one written first accepted unknown fields, so it would have
    // passed exactly the manifests the real loader rejects.
    let scaffold: BTreeSet<String> = {
        let templates = patterns_dir().parent().unwrap().to_path_buf();
        harness_core::scaffold::ScaffoldManifest::load(&templates)
            .expect("scaffold.toml loads")
            .tier(harness_core::scaffold::Tier::Foundation)
            .filter_map(|a| skill_dir_of(&a.destination).map(str::to_string))
            .collect()
    };

    let mut checked = 0usize;
    for pattern in &load_manifest().pattern {
        let mut dirs: std::collections::BTreeMap<&str, Vec<&String>> =
            std::collections::BTreeMap::new();
        for file in &pattern.files {
            if let Some(dir) = skill_dir_of(&file.destination) {
                dirs.entry(dir).or_default().push(&file.destination);
            }
        }
        for (dir, files) in &dirs {
            if scaffold.contains(*dir) {
                continue;
            }
            let heads = files
                .iter()
                .filter(|d| Surface::of(d) == Some(Surface::Skill))
                .count();
            assert_eq!(
                heads, 1,
                "pattern '{}' writes into .claude/skills/{dir}/ and declares {heads} entry \
                 points there, among {files:?}. Installed alone — which is how a pattern \
                 installs — that leaves a skill Claude Code does not load, or a resource \
                 belonging to no skill. The scaffold's own skill directories are exempt \
                 because the scaffold emits them first.",
                pattern.slug
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "the pattern library must ship a skill of its own"
    );
}

/// No two patterns claim the same skill directory.
///
/// The companion test asks whether a pattern's own entry point is there when it
/// installs alone. This asks the other half — whether two patterns collide when
/// both are installed — and the two are not the same question. Grouping per
/// pattern to answer the first silently gave up the second: two patterns each
/// declaring `.claude/skills/shared/SKILL.md` each hold exactly one entry point
/// in their own bucket, so the per-pattern check passes and one file overwrites
/// the other in any project that takes both.
///
/// Short directory names make this reachable rather than exotic — `spec` and
/// `review` are already taken, and the ninth pattern picks from the same small
/// vocabulary.
#[test]
fn no_two_patterns_claim_the_same_skill_directory() {
    let mut owner: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    let manifest = load_manifest();
    for pattern in &manifest.pattern {
        for file in &pattern.files {
            if Surface::of(&file.destination) == Some(Surface::Skill)
                && let Some(dir) = skill_dir_of(&file.destination)
            {
                owner.entry(dir).or_default().push(&pattern.slug);
            }
        }
    }
    for (dir, patterns) in &owner {
        assert_eq!(
            patterns.len(),
            1,
            "patterns {patterns:?} each declare an entry point at .claude/skills/{dir}/. \
             Installing both writes one over the other, and the project keeps whichever \
             ran last."
        );
    }
}

/// The skill directory a destination lands in, if it lands in one.
///
/// Four segments exactly, matching `Surface::of` — a deeper path is not a skill
/// layout Claude Code loads, and that test panics on it rather than this one
/// silently ignoring it.
fn skill_dir_of(destination: &str) -> Option<&str> {
    match *destination.split('/').collect::<Vec<_>>().as_slice() {
        [".claude", "skills", dir, _] => Some(dir),
        _ => None,
    }
}
