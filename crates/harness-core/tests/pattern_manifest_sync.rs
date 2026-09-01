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

/// Every manifest slug has its analysis entry in `reference/patterns.md`, and
/// every backtick-quoted bullet lead there is a manifest slug. The reference
/// is a projection of the manifest (Constitution IX): an added pattern with
/// no analysis entry ships the blank-page problem, and an entry for a
/// removed pattern instructs an install that cannot happen.
#[test]
fn reference_patterns_doc_mirrors_the_manifest() {
    let manifest = load_manifest();
    let doc_path = patterns_dir().join("../../reference/patterns.md");
    let doc = std::fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", doc_path.display()));

    let mut doc_slugs = BTreeSet::new();
    for line in doc.lines() {
        if let Some(rest) = line.strip_prefix("- `")
            && let Some((slug, _)) = rest.split_once('`')
        {
            doc_slugs.insert(slug.to_string());
        }
    }
    let manifest_slugs: BTreeSet<String> =
        manifest.pattern.iter().map(|p| p.slug.clone()).collect();
    assert_eq!(
        manifest_slugs, doc_slugs,
        "reference/patterns.md entries drifted from manifest.toml slugs"
    );
}

/// The review grammar's owner is `harness_core::plan` — the computer — and
/// the template prose that teaches it is a projection. Every file that states
/// the disposition vocabulary spells each token in the form the parser reads,
/// `[<disposition>: …]`, so renaming a token in one place only, or teaching a
/// spelling the computer rejects, fails here.
#[test]
fn disposition_vocabulary_is_stated_identically() {
    for rel in [
        "spec-workflow/skill/gates.md",
        "spec-workflow/specs/plan.md",
        "review-lenses/skill/convergence.md",
    ] {
        let path = patterns_dir().join(rel);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for token in harness_core::plan::Disposition::ALL {
            assert!(
                text.contains(&format!("`[{}:", token.as_str())),
                "{rel} does not spell `[{}: …]` — the prose teaches the grammar the computer \
                 reads, in the computer's own form",
                token.as_str()
            );
        }
    }
}

/// The example decision line gates.md ships parses under the shipped parser,
/// with the counts and gate the prose beside it describes. A doc whose own
/// example the computer rejects is teaching a grammar that does not exist.
#[test]
fn the_gates_example_line_parses_under_the_shipped_grammar() {
    let text = std::fs::read_to_string(patterns_dir().join("spec-workflow/skill/gates.md"))
        .expect("read gates.md");
    let example = text
        .lines()
        .find_map(|l| l.strip_prefix("- ").filter(|rest| rest.contains(" · ")))
        .expect("gates.md carries an example decision bullet");
    let line = harness_core::plan::parse_decision(example)
        .unwrap_or_else(|| panic!("the gates.md example does not parse: {example}"));
    assert!(
        harness_core::plan::REVIEW_CLASS_GATES.contains(&line.gate.as_str()),
        "the example fires a review-class gate"
    );
    assert!(
        line.counts.is_some(),
        "the example carries the counts the prose demands of a review-class firing"
    );
}

/// Both review-class gates the parser binds the counts contract to are the
/// gates gates.md documents — the constant and the doc name one set.
#[test]
fn the_review_class_gates_are_the_ones_the_doc_documents() {
    let text = std::fs::read_to_string(patterns_dir().join("spec-workflow/skill/gates.md"))
        .expect("read gates.md");
    for gate in harness_core::plan::REVIEW_CLASS_GATES {
        assert!(
            text.lines()
                .any(|l| l.trim_end().starts_with(&format!("## {gate} "))),
            "gates.md documents no `## {gate}` event, yet the parser holds it to the counts \
             contract"
        );
    }
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
    /// A scheduled routine.
    Routine,
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
    /// A GitHub template, a hook script, or a git pre-commit arm: none is a
    /// Claude Code surface.
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
    ///
    /// Asking is `V::covers`, not a `Pattern::matches` over `V::GLOB`: the
    /// string alone does not carry discovery's semantics, and matching it with
    /// the default options lets a `*` cross a separator. That read this
    /// classifier's own escapes as covered — a `SKILL.md` nested a level too
    /// deep classified as a skill while `skill_dir_of` saw no directory — and
    /// it left the round that introduced it with a mutation test that could not
    /// fail, since `**` and `*` had become the same thing.
    fn of(destination: &str) -> Option<Self> {
        use harness_core::validate::{
            AgentValidator, OutputStyleValidator, RoutineValidator, RuleValidator, SkillValidator,
            SurfaceValidator,
        };
        if SkillValidator::covers(destination) {
            return Some(Self::Skill);
        }
        if AgentValidator::covers(destination) {
            return Some(Self::Agent);
        }
        if RuleValidator::covers(destination) {
            return Some(Self::Rule);
        }
        if RoutineValidator::covers(destination) {
            return Some(Self::Routine);
        }
        if OutputStyleValidator::covers(destination) {
            return Some(Self::OutputStyle);
        }
        let seg: Vec<&str> = destination.split('/').collect();
        Some(match seg.as_slice() {
            [".claude", "skills", _, f] if f.ends_with(".md") => Self::SkillResource,
            [".claude", "lenses", f] if f.ends_with(".md") => Self::Lens,
            ["specs", "_template", f] if f.ends_with(".md") => Self::SpecTemplate,
            [".github", "pull_request_template.md"] => Self::OutsideClaudeCode,
            ["hooks", f] if f.ends_with(".sh") => Self::OutsideClaudeCode,

            ["hooks", "pre-commit.d", f] if f.ends_with(".sh") => Self::OutsideClaudeCode,
            _ => return None,
        })
    }
}

#[test]
fn every_pattern_surface_file_validates() {
    use harness_core::validate::{
        AgentValidator, OutputStyleValidator, RoutineValidator, RuleValidator, SkillValidator,
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
    let routines = validate
        .routines
        .expect("scaffolded config declares validate.routines");

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
                Surface::Routine => {
                    seen.insert("routine");
                    RoutineValidator::new(&routines).validate_text(&body, &landed)
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
    let known: BTreeSet<&str> = ["agent", "output-style", "routine", "rule", "skill"]
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
/// Skill directories the scaffold emits before any pattern runs.
fn foundation_skill_dirs() -> BTreeSet<String> {
    // Through the crate's own loader, not a second parser. `scaffold.toml` is a
    // closed schema and `ScaffoldManifest::load` is what enforces that.
    let templates = patterns_dir().parent().unwrap().to_path_buf();
    harness_core::scaffold::ScaffoldManifest::load(&templates)
        .expect("scaffold.toml loads")
        .tier(harness_core::scaffold::Tier::Foundation)
        .filter_map(|a| skill_dir_of(&a.destination).map(str::to_string))
        .collect()
}

/// A pattern is the install unit, so its own entry point must be there when it
/// installs alone. `Err` names what is wrong.
///
/// Pure over the manifest so the escapes found in earlier rounds are unit tests
/// below rather than mutations someone has to remember to re-run. That is the
/// check the round-four regression needed and did not get: each round's fix ran
/// its own new reproduction and not the previous ones.
fn entry_point_available_at_install(
    patterns: &[Pattern],
    foundation: &BTreeSet<String>,
) -> Result<usize, String> {
    let mut checked = 0usize;
    for pattern in patterns {
        let mut dirs: std::collections::BTreeMap<&str, Vec<&String>> =
            std::collections::BTreeMap::new();
        for file in &pattern.files {
            if let Some(dir) = skill_dir_of(&file.destination) {
                dirs.entry(dir).or_default().push(&file.destination);
            }
        }
        for (dir, files) in &dirs {
            if foundation.contains(*dir) {
                continue;
            }
            let heads = files
                .iter()
                .filter(|d| Surface::of(d) == Some(Surface::Skill))
                .count();
            if heads != 1 {
                return Err(format!(
                    "pattern '{}' writes into .claude/skills/{dir}/ and declares {heads} entry \
                     points there, among {files:?}. Installed alone — which is how a pattern \
                     installs — that leaves a skill Claude Code does not load, or a resource \
                     belonging to no skill. The scaffold's own skill directories are exempt \
                     because the scaffold emits them first.",
                    pattern.slug
                ));
            }
            checked += 1;
        }
    }
    Ok(checked)
}

/// Two patterns installed together must not write over each other's skill.
/// A different question from the one above, and it needs its own answer.
fn no_shared_skill_directory(patterns: &[Pattern]) -> Result<(), String> {
    let mut owner: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    for pattern in patterns {
        for file in &pattern.files {
            if Surface::of(&file.destination) == Some(Surface::Skill)
                && let Some(dir) = skill_dir_of(&file.destination)
            {
                owner.entry(dir).or_default().push(&pattern.slug);
            }
        }
    }
    for (dir, slugs) in &owner {
        if slugs.len() != 1 {
            return Err(format!(
                "patterns {slugs:?} each declare an entry point at .claude/skills/{dir}/. \
                 Installing both writes one over the other, and the project keeps whichever \
                 ran last."
            ));
        }
    }
    Ok(())
}

#[test]
fn every_skill_directory_has_its_entry_point_at_install_time() {
    let checked =
        entry_point_available_at_install(&load_manifest().pattern, &foundation_skill_dirs())
            .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        checked > 0,
        "the pattern library must ship a skill of its own"
    );
}

/// No two patterns claim the same skill directory.
///
/// The companion test asks whether a pattern's own entry point is there when it
/// installs alone. This asks the other half — whether two patterns collide when
/// both are installed. Grouping per pattern to answer the first silently gave up
/// the second, and nothing noticed for a round.
///
/// Short directory names make this reachable rather than exotic — `spec` and
/// `review` are already taken, and the ninth pattern picks from the same small
/// vocabulary.
#[test]
fn no_two_patterns_claim_the_same_skill_directory() {
    no_shared_skill_directory(&load_manifest().pattern).unwrap_or_else(|e| panic!("{e}"));
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

/// Every cross-reference `body` makes, as (target as written, destination it
/// resolves to). `from` is the destination the prose itself lands at.
///
/// Two grammars and only two, each unambiguous so the check never guesses: an
/// inline link, resolved against the directory `from` lands in; and an inline
/// code span holding a concrete project-relative path, which is already a
/// destination. A glob, a `{param}`, a `<placeholder>`, or prose inside a span
/// is not a path and is not read as one — those are the forms this library
/// actually writes, and reading them as paths is the false positive that would
/// make the check unusable. Fenced blocks are examples, not references.
fn cross_references(from: &str, body: &str) -> Vec<(String, String)> {
    let mut refs = Vec::new();
    let mut fenced = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut rest = line;
        while let Some(open) = rest.find("](") {
            let after = &rest[open + 2..];
            let Some(close) = after.find(')') else { break };
            if let Some(resolved) = resolve_relative(from, &after[..close]) {
                refs.push((after[..close].to_string(), resolved));
            }
            rest = &after[close + 1..];
        }
        let mut rest = line;
        while let Some(open) = rest.find('`') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('`') else { break };
            let span = &after[..close];
            if is_project_path(span) {
                refs.push((span.to_string(), span.to_string()));
            }
            rest = &after[close + 1..];
        }
    }
    refs
}

/// The destination a link written inside `from` points at, or `None` when the
/// target is not a relative reference to a markdown file.
fn resolve_relative(from: &str, target: &str) -> Option<String> {
    let path = target.split('#').next().unwrap_or(target);
    if path.is_empty() || path.contains("://") || path.starts_with('/') || !path.ends_with(".md") {
        return None;
    }
    let mut segments: Vec<&str> = from.split('/').collect();
    segments.pop();
    for segment in path.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}

/// Whether a code span is a concrete project-relative path to a markdown file.
fn is_project_path(span: &str) -> bool {
    span.contains('/')
        && span.ends_with(".md")
        && !span
            .chars()
            .any(|c| c.is_whitespace() || "*<>{}".contains(c))
}

/// Every destination the scaffold emits before any pattern runs.
fn foundation_destinations() -> BTreeSet<String> {
    let templates = patterns_dir().parent().unwrap().to_path_buf();
    harness_core::scaffold::ScaffoldManifest::load(&templates)
        .expect("scaffold.toml loads")
        .tier(harness_core::scaffold::Tier::Foundation)
        .map(|a| a.destination.clone())
        .collect()
}

/// Prose a pattern ships may only point at a file the reader will have.
///
/// A pattern installs alone, so its own destinations plus the foundation tier
/// are the whole of what it may name. `Err` says which reference dangles.
fn cross_references_resolve(
    patterns: &[Pattern],
    foundation: &BTreeSet<String>,
    body: impl Fn(&str, &str) -> String,
) -> Result<usize, String> {
    let mut checked = 0usize;
    for pattern in patterns {
        let installed: BTreeSet<&str> = pattern
            .files
            .iter()
            .map(|f| f.destination.as_str())
            .collect();
        for file in &pattern.files {
            if !file.destination.ends_with(".md") {
                continue;
            }
            for (written, resolved) in
                cross_references(&file.destination, &body(&pattern.slug, &file.template))
            {
                if !installed.contains(resolved.as_str()) && !foundation.contains(&resolved) {
                    return Err(format!(
                        "pattern '{}' ships {} naming '{written}', which resolves to \
                         '{resolved}' — a destination neither this pattern nor the foundation \
                         tier installs. A pattern installs alone, so a reader following that \
                         reference finds nothing.",
                        pattern.slug, file.destination
                    ));
                }
                checked += 1;
            }
        }
    }
    Ok(checked)
}

/// A cross-reference in shipped prose resolves to a file the reader will have.
///
/// These references are what single-ownership costs: prose that would restate a
/// rule names the owner instead, and the name is worth no more than the file it
/// still points at. Nothing else in the suite reads them, so a renamed resource
/// left a dangling pointer in every install and stayed green.
#[test]
fn every_cross_reference_in_shipped_prose_resolves() {
    let manifest = load_manifest();
    let checked = cross_references_resolve(
        &manifest.pattern,
        &foundation_destinations(),
        |slug, template| {
            let path = patterns_dir().join(slug).join(template);
            std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        checked > 0,
        "the pattern library must cross-reference at least one of its own files"
    );
}

/// The escapes found in rounds three through six, as tests rather than as
/// mutations someone has to remember to re-run.
///
/// Every one of these was found by editing `manifest.toml` by hand and watching
/// the suite stay green. Each round then fixed its own escape and ran only its
/// own reproduction — which is how round four closed one hole and reopened
/// another that round three had closed, with nothing failing for a full round.
/// Held here, giving one up is a failing test rather than a discovery two
/// rounds later.
mod escapes {
    use super::*;

    fn pattern(slug: &str, destinations: &[&str]) -> Pattern {
        Pattern {
            slug: slug.to_string(),
            analyze: vec!["something".into()],
            files: destinations
                .iter()
                .map(|d| FileEntry {
                    template: "t.md".into(),
                    destination: (*d).to_string(),
                })
                .collect(),
        }
    }

    fn none() -> BTreeSet<String> {
        BTreeSet::new()
    }

    fn prose(slug: &str, files: &[(&str, &str)]) -> Pattern {
        Pattern {
            slug: slug.to_string(),
            analyze: vec!["something".into()],
            files: files
                .iter()
                .map(|(template, destination)| FileEntry {
                    template: (*template).to_string(),
                    destination: (*destination).to_string(),
                })
                .collect(),
        }
    }

    fn body_of(entries: &'static [(&'static str, &'static str)]) -> impl Fn(&str, &str) -> String {
        move |_slug: &str, template: &str| {
            entries
                .iter()
                .find(|(t, _)| *t == template)
                .map(|(_, b)| (*b).to_string())
                .unwrap_or_default()
        }
    }

    fn scaffold_ships(dir: &str) -> BTreeSet<String> {
        [dir.to_string()].into_iter().collect()
    }

    /// Round 5: two patterns racing for one skill directory. Round 4's
    /// per-pattern grouping accepted this — each holds exactly one entry point
    /// in its own bucket — and installing both writes one over the other.
    #[test]
    fn two_patterns_may_not_share_a_skill_directory() {
        let ps = [
            pattern("alpha", &[".claude/skills/shared/SKILL.md"]),
            pattern("beta", &[".claude/skills/shared/SKILL.md"]),
        ];
        assert!(no_shared_skill_directory(&ps).is_err());
        // and the per-pattern check alone does NOT catch it — the reason both exist
        assert!(entry_point_available_at_install(&ps, &none()).is_ok());
    }

    /// Round 4: a pattern writing a resource into another pattern's skill
    /// directory. Installed alone it leaves a resource belonging to no skill.
    #[test]
    fn a_pattern_may_not_write_into_another_patterns_skill_directory() {
        let ps = [
            pattern("owner", &[".claude/skills/spec/SKILL.md"]),
            pattern("guest", &[".claude/skills/spec/extra.md"]),
        ];
        assert!(entry_point_available_at_install(&ps, &none()).is_err());
    }

    /// Round 3: a one-character filename typo. The directory then has no entry
    /// point, and Claude Code loads nothing from it.
    #[test]
    fn a_skill_entry_point_filename_typo_is_caught() {
        let ps = [pattern(
            "spec-workflow",
            &[
                ".claude/skills/spec/Skill.md",
                ".claude/skills/spec/gates.md",
            ],
        )];
        assert!(entry_point_available_at_install(&ps, &none()).is_err());
    }

    /// A skill directory shipping only resources.
    #[test]
    fn a_skill_directory_needs_an_entry_point() {
        let ps = [pattern("p", &[".claude/skills/spec/gates.md"])];
        assert!(entry_point_available_at_install(&ps, &none()).is_err());
    }

    /// Two entry points in one directory: the second overwrites the first.
    #[test]
    fn a_skill_directory_holds_only_one_entry_point() {
        let ps = [pattern(
            "p",
            &[
                ".claude/skills/spec/SKILL.md",
                ".claude/skills/spec/SKILL.md",
            ],
        )];
        assert!(entry_point_available_at_install(&ps, &none()).is_err());
    }

    /// Extending a skill directory the scaffold emits is legitimate: the
    /// foundation tier runs before any pattern, so the entry point is there.
    #[test]
    fn extending_a_scaffold_skill_directory_is_allowed() {
        let ps = [pattern("p", &[".claude/skills/harness-curate/extra.md"])];
        assert!(entry_point_available_at_install(&ps, &scaffold_ships("harness-curate")).is_ok());
    }

    /// A skill with an entry point and no resources at all.
    #[test]
    fn a_skill_with_no_resources_is_allowed() {
        let ps = [pattern("p", &[".claude/skills/solo/SKILL.md"])];
        assert!(entry_point_available_at_install(&ps, &none()).unwrap() == 1);
    }

    /// Round 6: the validators discover recursively, so a nested rule or agent
    /// is a destination the oracle validates and the classifier must accept.
    #[test]
    fn the_classifier_accepts_what_the_validators_cover() {
        for (destination, expected) in [
            (".claude/rules/observability.md", Surface::Rule),
            (".claude/rules/nested/observability.md", Surface::Rule),
            (".claude/agents/reviewer.md", Surface::Agent),
            (".claude/agents/team/reviewer.md", Surface::Agent),
            (".claude/output-styles/terse.md", Surface::OutputStyle),
            (".claude/skills/spec/SKILL.md", Surface::Skill),
            (".claude/skills/spec/gates.md", Surface::SkillResource),
        ] {
            assert_eq!(
                Surface::of(destination),
                Some(expected),
                "{destination} classified wrongly"
            );
        }
    }

    /// Round 7: the classifier and `skill_dir_of` must answer the same question
    /// the same way.
    ///
    /// `entry_point_available_at_install` counts entry points among the files
    /// `skill_dir_of` places in a directory, so a destination the classifier
    /// calls a skill and `skill_dir_of` does not place is a skill that check
    /// never sees — it returns "nothing to check", which reads exactly like a
    /// pattern shipping no skill at all. That was live: a `SKILL.md` nested one
    /// directory too deep, which Claude Code loads no skill from, matched
    /// `.claude/skills/*/SKILL.md` because the classifier read the glob with a
    /// `*` that crossed separators.
    #[test]
    fn every_skill_the_classifier_names_has_a_skill_directory() {
        for destination in [
            ".claude/skills/spec/SKILL.md",
            ".claude/skills/spec/sub/SKILL.md",
            ".claude/skills/spec/gates.md",
            ".claude/skills/a/b/c/SKILL.md",
            ".claude/rules/nested/x.md",
        ] {
            if Surface::of(destination) == Some(Surface::Skill) {
                assert!(
                    skill_dir_of(destination).is_some(),
                    "{destination} classifies as a skill entry point that no \
                     skill directory holds, so the install-time check skips it"
                );
            }
        }
        assert_eq!(Surface::of(".claude/skills/spec/sub/SKILL.md"), None);
    }

    /// A destination no surface covers fails loudly rather than passing unseen.
    #[test]
    fn an_unclassifiable_destination_is_not_silently_excused() {
        for destination in [".claude/hooks/x.sh", "README.md", ".claude/skills/x/y/z.md"] {
            assert_eq!(
                Surface::of(destination),
                None,
                "{destination} was classified"
            );
        }
    }

    /// A link naming a resource the pattern stopped shipping. Every install
    /// carries the dangling pointer and nothing else in the suite looks.
    #[test]
    fn a_reference_to_a_file_the_pattern_does_not_install_is_caught() {
        let ps = [prose(
            "spec-workflow",
            &[("skill/SKILL.md", ".claude/skills/spec/SKILL.md")],
        )];
        let body = body_of(&[("skill/SKILL.md", "Read [resume.md](resume.md) on a resume.")]);
        assert!(cross_references_resolve(&ps, &none(), body).is_err());
    }

    /// Two files side by side in the template tree, landing in different
    /// directories. Resolved in template space the link looks fine; the reader
    /// is in the installed tree, where it dangles.
    #[test]
    fn a_link_resolved_in_template_space_is_not_the_readers_link() {
        let ps = [prose(
            "spec-workflow",
            &[
                ("skill/SKILL.md", ".claude/skills/spec/SKILL.md"),
                ("spec-workflow.md", ".claude/rules/spec-workflow.md"),
            ],
        )];
        assert!(
            cross_references_resolve(
                &ps,
                &none(),
                body_of(&[("skill/SKILL.md", "The rule is [it](spec-workflow.md).")])
            )
            .is_err()
        );
        assert!(
            cross_references_resolve(
                &ps,
                &none(),
                body_of(&[(
                    "skill/SKILL.md",
                    "The rule is [it](../../rules/spec-workflow.md).",
                )])
            )
            .is_ok()
        );
    }

    /// Naming an owner by its project-relative path is the other half of the
    /// same reference, and it dangles the same way.
    #[test]
    fn a_backticked_path_is_a_reference_too() {
        let ps = [prose("p", &[("s.md", ".claude/skills/review/SKILL.md")])];
        let body = body_of(&[(
            "s.md",
            "The table in `.claude/rules/review-lenses.md` decides.",
        )]);
        assert!(cross_references_resolve(&ps, &none(), body).is_err());
    }

    /// The foundation tier is installed before any pattern runs, so prose may
    /// point at it.
    #[test]
    fn a_reference_to_a_foundation_file_is_allowed() {
        let ps = [prose("p", &[("s.md", ".claude/skills/spec/SKILL.md")])];
        let foundation: BTreeSet<String> = [".claude/rules/constitution.md".to_string()]
            .into_iter()
            .collect();
        let body = body_of(&[(
            "s.md",
            "See [it](../../rules/constitution.md) and `.claude/rules/constitution.md`.",
        )]);
        assert!(cross_references_resolve(&ps, &foundation, body).is_ok());
    }

    /// The forms this library writes that are not paths. Reading any of them as
    /// one is the false positive that would make the check unusable, so each is
    /// pinned rather than left to the grammar.
    #[test]
    fn a_glob_a_placeholder_and_prose_are_not_references() {
        let ps = [prose("p", &[("s.md", ".claude/skills/spec/SKILL.md")])];
        let body = body_of(&[(
            "s.md",
            "`.claude/rules/*.md` and `.claude/lenses/<id>.md` and \
             `.claude/rules/{lang}-conventions.md` and `plan.md ## Outstanding issues` \
             and [docs](https://example.invalid/x.md) and [run](run.sh) and [a](#anchor)\n\
             ```\n\
             `.claude/skills/gone/SKILL.md`\n\
             ```",
        )]);
        assert_eq!(cross_references_resolve(&ps, &none(), body).unwrap(), 0);
    }

    /// A `..` climbing past the project root resolves to nothing rather than to
    /// a path that happens to match.
    #[test]
    fn a_reference_climbing_past_the_root_is_not_resolved() {
        assert_eq!(
            resolve_relative(".claude/skills/spec/SKILL.md", "../../../../escape.md"),
            None
        );
    }
}
