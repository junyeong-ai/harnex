//! # governs — what a rule is truth about
//!
//! A rule's `paths:` frontmatter says when it LOADS; its `governs:` block
//! says what it is truth ABOUT. The two diverge exactly where the mechanism
//! earns its place: a naming rule loads beside the scaffold scripts it is
//! read with and governs the manifest those scripts write. One declaration,
//! three consumers — review scope resolution ([`resolve`]: file → the rules
//! that govern it), staleness ([`GovernsAuditor`]: a declared truth that no
//! longer exists), and the completeness gate
//! ([`crate::validate::rules`], which consumes [`GovernsDecl::from_rule`]).
//!
//! The declaration is a closed mapping (`concept` + `live_truth` +
//! optional `decision_record`); `live_truth` entries are literal
//! project-relative paths, where a directory covers its subtree.
//!
//! ## What this module refuses to do
//!
//! - **Never infer governance.** A rule without `governs:` governs nothing
//!   here — deriving governance from prose, file names, or `paths:` globs is
//!   a heuristic with a false-positive floor, and a wrong governance edge
//!   pulls the wrong rule into every review.
//! - **Never glob.** A `live_truth` entry is a literal path, so two
//!   evaluators — this resolver and a reader — cannot disagree about what it
//!   covers. An entry carrying `*`, `?` or `[` is a shape error at parse.
//! - **Never read git.** Whether a truth *churned* is history; this module
//!   answers only whether it *exists*. History belongs to the caller.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::validate::frontmatter;

/// A rule's parsed `governs:` declaration.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct GovernsDecl {
    /// What the rule governs, in the project's own words.
    pub concept: String,
    /// Project-relative paths the rule is truth about. A directory entry
    /// covers its subtree.
    pub live_truth: Vec<String>,
    /// Where the decision behind the rule is recorded, when anywhere.
    pub decision_record: Option<String>,
}

#[derive(Deserialize)]
struct GovernsFrontmatter {
    #[serde(default)]
    governs: Option<RawDecl>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDecl {
    concept: String,
    live_truth: yaml_serde::Value,
    #[serde(default)]
    decision_record: Option<String>,
}

/// `live_truth` as entries, with the shape stated in the diagnostic — a
/// derive over an untagged enum names its own variants instead of the
/// grammar the author must fix.
fn truth_entries(value: yaml_serde::Value) -> std::result::Result<Vec<String>, ShapeError> {
    const SHAPE: &str = "`live_truth` must be a string or a list of strings";
    match value {
        yaml_serde::Value::String(s) => Ok(vec![s]),
        yaml_serde::Value::Sequence(seq) => seq
            .into_iter()
            .map(|v| match v {
                yaml_serde::Value::String(s) => Ok(s),
                _ => Err(ShapeError::Malformed(SHAPE.into())),
            })
            .collect(),
        _ => Err(ShapeError::Malformed(SHAPE.into())),
    }
}

/// How a `governs:` block failed to be a declaration. The validator maps
/// each to a finding; this module never speaks in findings for shape.
#[derive(Debug, PartialEq, Eq)]
pub enum ShapeError {
    /// The block does not deserialize: a missing required key, an unknown
    /// key, or a value of the wrong type.
    Malformed(String),
    /// `concept` is empty or whitespace.
    EmptyConcept,
    /// `live_truth` carries no entries.
    EmptyLiveTruth,
    /// An entry is not a literal project-relative path: empty, absolute,
    /// traversing (`.`/`..`), or carrying glob metacharacters.
    BadPath(String),
}

impl std::fmt::Display for ShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(msg) => write!(f, "`governs:` does not parse: {msg}"),
            Self::EmptyConcept => write!(f, "`governs.concept` is empty"),
            Self::EmptyLiveTruth => write!(f, "`governs.live_truth` declares no paths"),
            Self::BadPath(p) => write!(
                f,
                "`governs` path '{p}' is not a literal project-relative path"
            ),
        }
    }
}

/// Why a rule's declaration could not be read at all — split from
/// [`ShapeError`] because the two halves have different owners inside
/// `check` (frontmatter is the rule validator's Blocker) while a standalone
/// reader reports both.
#[derive(Debug)]
pub enum DeclError {
    /// The file's frontmatter does not parse, so whether it declares is
    /// unknowable.
    Frontmatter(String),
    Shape(ShapeError),
}

impl std::fmt::Display for DeclError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Frontmatter(msg) => write!(f, "frontmatter does not parse: {msg}"),
            Self::Shape(e) => e.fmt(f),
        }
    }
}

/// Whether a declared path is a literal project-relative path. Globs are
/// refused by the module contract; traversal and absolute paths would let a
/// declaration reach outside the project it describes; a backslash is
/// refused because `/` is the one separator this grammar reads, and a path
/// that resolves differently per platform is not literal.
fn literal_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['*', '?', '[', '\\', '\0'])
        && !path
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
}

fn normalize(path: &str) -> &str {
    path.strip_suffix('/').unwrap_or(path)
}

impl GovernsDecl {
    /// The `governs:` declaration in a rule's frontmatter YAML, when present.
    ///
    /// `Ok(None)` is a rule that declares nothing — a fact for the caller,
    /// never an error here. Every other deviation is a [`ShapeError`].
    pub fn from_yaml(yaml_text: &str) -> std::result::Result<Option<Self>, ShapeError> {
        let parsed: GovernsFrontmatter =
            yaml_serde::from_str(yaml_text).map_err(|e| ShapeError::Malformed(e.to_string()))?;
        let Some(raw) = parsed.governs else {
            return Ok(None);
        };
        if raw.concept.trim().is_empty() {
            return Err(ShapeError::EmptyConcept);
        }
        let live_truth: Vec<String> = truth_entries(raw.live_truth)?
            .into_iter()
            .map(|p| normalize(&p).to_string())
            .collect();
        if live_truth.is_empty() {
            return Err(ShapeError::EmptyLiveTruth);
        }
        for path in &live_truth {
            if !literal_relative(path) {
                return Err(ShapeError::BadPath(path.clone()));
            }
        }
        if let Some(record) = &raw.decision_record
            && !literal_relative(normalize(record))
        {
            return Err(ShapeError::BadPath(record.clone()));
        }
        Ok(Some(Self {
            concept: raw.concept.trim().to_string(),
            live_truth,
            decision_record: raw.decision_record.map(|r| normalize(&r).to_string()),
        }))
    }

    /// The declaration in a whole rule file's text. A file with no
    /// frontmatter declares nothing; unterminated frontmatter and a
    /// malformed declaration are each their own error, because a caller with
    /// no validator beside it (the resolver) must be able to say which rules
    /// it could not read rather than fold them into silence.
    pub fn from_rule(content: &str, source: &Path) -> std::result::Result<Option<Self>, DeclError> {
        match frontmatter::parse(content, source) {
            Ok(Some(fm)) => Self::from_yaml(&fm.yaml_text).map_err(DeclError::Shape),
            Ok(None) => Ok(None),
            Err(e) => Err(DeclError::Frontmatter(e.to_string())),
        }
    }

    /// Whether this declaration covers `path` — an entry equals it, or is an
    /// ancestor directory of it. Component-wise, so `src/foo` never covers
    /// `src/foobar`.
    pub fn covers(&self, path: &str) -> bool {
        let path = normalize(path);
        self.live_truth.iter().any(|entry| {
            path == entry
                || (path.len() > entry.len()
                    && path.as_bytes()[entry.len()] == b'/'
                    && path.starts_with(entry.as_str()))
        })
    }
}

/// A query path in the form [`resolve`] compares: project-relative and
/// `./`-free, the same grammar declarations are held to at parse.
///
/// An absolute path under `root` is the same question spelled from
/// elsewhere, so it is stripped; one outside the root, or traversing
/// upward, is unanswerable and refused — a silent empty answer would read
/// as "governed by nothing", which is a different fact.
pub fn normalize_query(root: &Path, query: &str) -> Result<String> {
    let q = Path::new(query);
    let rel = if q.is_absolute() {
        q.strip_prefix(root).map_err(|_| Error::PathTraversal {
            path: q.to_path_buf(),
        })?
    } else {
        q
    };
    let mut parts: Vec<&str> = Vec::new();
    for component in rel.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(seg) => {
                // `rel` borrows from a `&str`, so every segment is UTF-8.
                parts.push(seg.to_str().expect("segments of a str-backed path"));
            }
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(Error::PathTraversal {
                    path: q.to_path_buf(),
                });
            }
        }
    }
    Ok(parts.join("/"))
}

/// One rule and what it declares, as [`load`] reads it off the tree.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct RuleGoverns {
    /// The rule file, project-relative.
    pub rule: String,
    pub governs: GovernsDecl,
}

/// A rule whose declaration could not be read, and why — carried beside the
/// resolution so a standalone consumer sees the narrowed scope instead of a
/// clean-looking answer.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct LoadDefect {
    /// The rule file, project-relative.
    pub rule: String,
    pub error: String,
}

/// What [`load`] read off the tree: the well-shaped declarations, and the
/// rules it had to exclude.
#[derive(Debug, Clone)]
pub struct LoadOutcome {
    pub rules: Vec<RuleGoverns>,
    pub defects: Vec<LoadDefect>,
}

/// Every rule under `root`, read for its `governs:` declaration.
///
/// A defective declaration never resolves — a resolver that guessed at a
/// malformed one would resolve differently than the gate reads — but it is
/// returned as a [`LoadDefect`], never dropped: the resolver runs without
/// the validator beside it, and an exclusion nothing reports is silent
/// scope shrinkage. An unreadable file or a failed traversal is an error,
/// exactly as `check` treats the same class.
pub fn load(root: &Path) -> Result<LoadOutcome> {
    use crate::validate::{RuleValidator, SurfaceValidator};
    let pattern = crate::glob_root::rooted(root, <RuleValidator as SurfaceValidator>::GLOB)?;
    let mut paths = Vec::new();
    for entry in glob::glob(&pattern).map_err(|e| Error::ConfigInvalid {
        message: format!("rules glob: {e}"),
        location: None,
    })? {
        paths.push(entry.map_err(|e| {
            let path = e.path().to_path_buf();
            Error::IoFailure {
                path,
                source: e.into(),
            }
        })?);
    }
    paths.sort();
    let mut outcome = LoadOutcome {
        rules: Vec::new(),
        defects: Vec::new(),
    };
    for path in paths {
        let content = std::fs::read_to_string(&path).map_err(|e| Error::IoFailure {
            path: path.clone(),
            source: e,
        })?;
        let rule = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        match GovernsDecl::from_rule(&content, &path) {
            Ok(Some(governs)) => outcome.rules.push(RuleGoverns { rule, governs }),
            Ok(None) => {}
            Err(e) => outcome.defects.push(LoadDefect {
                rule,
                error: e.to_string(),
            }),
        }
    }
    Ok(outcome)
}

/// The rules governing one queried path.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Resolution {
    /// The queried path, as [`normalize_query`] spelled it.
    pub path: String,
    /// Rule files whose `governs.live_truth` covers it, in rule-path order.
    pub rules: Vec<String>,
}

/// For each queried path, the rules whose declared truth covers it.
///
/// A path no rule covers resolves to an empty list — an answer, not an
/// error: most files are governed by nothing beyond the always-loaded set.
pub fn resolve(rules: &[RuleGoverns], paths: &[String]) -> Vec<Resolution> {
    paths
        .iter()
        .map(|path| Resolution {
            path: path.clone(),
            rules: rules
                .iter()
                .filter(|r| r.governs.covers(path))
                .map(|r| r.rule.clone())
                .collect(),
        })
        .collect()
}

/// Cross-input check: every declared truth still exists in the tree.
///
/// Shape problems are deliberately not re-reported here — the rule validator
/// owns them, and `check` runs both, so a second copy would double every
/// finding.
pub struct GovernsAuditor<'a> {
    root: &'a Path,
}

impl<'a> GovernsAuditor<'a> {
    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }

    pub fn audit_rule(&self, content: &str, path: &Path) -> Vec<Finding> {
        let mut findings = Vec::new();
        let Ok(Some(governs)) = GovernsDecl::from_rule(content, path) else {
            return findings;
        };
        let mut declared: Vec<&String> = governs.live_truth.iter().collect();
        if let Some(record) = &governs.decision_record {
            declared.push(record);
        }
        for entry in declared {
            if !self.root.join(entry).exists() {
                findings.push(Finding {
                    slug: "governs-truth-missing".into(),
                    severity: Severity::Major,
                    location: Location::file(path.to_path_buf()),
                    message: format!("`governs` declares '{entry}', which does not exist"),
                    hint: Some(
                        "the declared truth moved or was deleted — re-point the declaration, \
                         or retire the rule that described it"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(yaml: &str) -> std::result::Result<Option<GovernsDecl>, ShapeError> {
        GovernsDecl::from_yaml(yaml)
    }

    #[test]
    fn a_full_declaration_parses_with_string_or_list_truth() {
        let one = decl("governs:\n  concept: naming\n  live_truth: pyproject.toml\n")
            .unwrap()
            .unwrap();
        assert_eq!(one.live_truth, vec!["pyproject.toml"]);
        let many = decl(
            "governs:\n  concept: naming\n  live_truth:\n    - src/\n    - pyproject.toml\n  \
             decision_record: docs/adr/001.md\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(many.live_truth, vec!["src", "pyproject.toml"]);
        assert_eq!(many.decision_record.as_deref(), Some("docs/adr/001.md"));
    }

    #[test]
    fn absence_is_none_not_an_error() {
        assert!(decl("paths:\n  - src/**\n").unwrap().is_none());
        assert!(decl("").unwrap().is_none());
    }

    #[test]
    fn each_shape_deviation_is_its_own_error() {
        assert!(matches!(
            decl("governs:\n  concept: x\n").unwrap_err(),
            ShapeError::Malformed(_)
        ));
        assert!(matches!(
            decl("governs:\n  concept: x\n  live_truth: src\n  extra: y\n").unwrap_err(),
            ShapeError::Malformed(_)
        ));
        assert_eq!(
            decl("governs:\n  concept: '  '\n  live_truth: src\n").unwrap_err(),
            ShapeError::EmptyConcept
        );
        assert_eq!(
            decl("governs:\n  concept: x\n  live_truth: []\n").unwrap_err(),
            ShapeError::EmptyLiveTruth
        );
        for bad in ["/abs", "a/../b", "src/*", "a//b", ".", "a\\b"] {
            assert_eq!(
                decl(&format!("governs:\n  concept: x\n  live_truth: '{bad}'\n")).unwrap_err(),
                ShapeError::BadPath(bad.to_string()),
                "{bad} accepted"
            );
        }
    }

    #[test]
    fn covers_is_equality_or_ancestry_never_prefix() {
        let g = decl("governs:\n  concept: x\n  live_truth: src/foo\n")
            .unwrap()
            .unwrap();
        assert!(g.covers("src/foo"));
        assert!(g.covers("src/foo/deep/file.rs"));
        assert!(g.covers("src/foo/"));
        assert!(!g.covers("src/foobar"));
        assert!(!g.covers("src"));
    }

    #[test]
    fn resolve_answers_empty_for_an_ungoverned_path() {
        let rules = vec![RuleGoverns {
            rule: ".claude/rules/naming.md".into(),
            governs: decl("governs:\n  concept: x\n  live_truth: src\n")
                .unwrap()
                .unwrap(),
        }];
        let out = resolve(&rules, &["src/a.rs".into(), "docs/x.md".into()]);
        assert_eq!(out[0].rules, vec![".claude/rules/naming.md"]);
        assert!(out[1].rules.is_empty());
    }

    #[test]
    fn load_reads_declaring_rules_and_reports_what_it_excludes() {
        let root = tempfile::tempdir().unwrap();
        let rules = root.path().join(".claude/rules");
        std::fs::create_dir_all(rules.join("nested")).unwrap();
        std::fs::write(
            rules.join("naming.md"),
            "---\ngoverns:\n  concept: naming\n  live_truth: src\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(
            rules.join("nested/deep.md"),
            "---\npaths: [\"src/**\"]\n---\n",
        )
        .unwrap();
        std::fs::write(rules.join("plain.md"), "no frontmatter\n").unwrap();
        std::fs::write(
            rules.join("bad.md"),
            "---\ngoverns:\n  concept: x\n  live_truth: 'src/*'\n---\n",
        )
        .unwrap();
        std::fs::write(rules.join("unterminated.md"), "---\npaths: [\"a/**\"]\n").unwrap();
        let outcome = load(root.path()).unwrap();
        assert_eq!(outcome.rules.len(), 1);
        assert_eq!(outcome.rules[0].rule, ".claude/rules/naming.md");
        let mut excluded: Vec<&str> = outcome.defects.iter().map(|d| d.rule.as_str()).collect();
        excluded.sort();
        assert_eq!(
            excluded,
            vec![".claude/rules/bad.md", ".claude/rules/unterminated.md"],
            "{:?}",
            outcome.defects
        );
    }

    #[test]
    fn a_query_is_normalized_to_the_grammar_declarations_are_held_to() {
        let root = Path::new("/proj");
        for (given, want) in [
            ("src/a.rs", "src/a.rs"),
            ("./src/a.rs", "src/a.rs"),
            ("src/./a.rs", "src/a.rs"),
            ("/proj/src/a.rs", "src/a.rs"),
            ("src/dir/", "src/dir"),
        ] {
            assert_eq!(normalize_query(root, given).unwrap(), want, "{given}");
        }
        for refused in ["/elsewhere/a.rs", "src/../..", "a/../b"] {
            assert!(
                matches!(
                    normalize_query(root, refused),
                    Err(Error::PathTraversal { .. })
                ),
                "{refused} accepted"
            );
        }
    }

    #[test]
    fn a_wrong_typed_live_truth_names_the_shape_it_wants() {
        for bad in [
            "governs:\n  concept: x\n  live_truth: [1, 2]\n",
            "governs:\n  concept: x\n  live_truth: {a: b}\n",
            "governs:\n  concept: x\n  live_truth:\n",
        ] {
            let err = decl(bad).unwrap_err();
            assert!(
                err.to_string()
                    .contains("must be a string or a list of strings"),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn auditor_reports_each_missing_truth_and_nothing_else() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        let auditor = GovernsAuditor::new(root.path());
        let content = "---\ngoverns:\n  concept: x\n  live_truth:\n    - src\n    - gone.rs\n  \
                       decision_record: docs/adr/001.md\n---\n";
        let findings = auditor.audit_rule(content, Path::new(".claude/rules/x.md"));
        let missing: Vec<&str> = findings.iter().map(|f| f.slug.as_str()).collect();
        assert_eq!(
            missing,
            vec!["governs-truth-missing", "governs-truth-missing"]
        );
        assert!(findings[0].message.contains("gone.rs"));
        assert!(findings[1].message.contains("docs/adr/001.md"));
        assert!(
            auditor
                .audit_rule("---\npaths: [\"src/**\"]\n---\n", Path::new("r.md"))
                .is_empty()
        );
    }
}
