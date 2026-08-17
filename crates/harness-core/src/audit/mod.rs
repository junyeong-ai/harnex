//! # audit — harness-engineering compliance gate
//!
//! Distinct from [`check`](crate::check), which validates structural
//! correctness, `audit` evaluates *engineering quality* of the generated
//! harness against spec-facts and the keep-soften-cut policy.
//!
//! Three classes of finding:
//!
//! - **Spec drift** — values that look plausible but violate the live
//!   Claude Code spec (millisecond `timeout`, incomplete `mcp__server`
//!   matcher).
//! - **Hook wiring** — a hook naming a scaffold artifact that is not on disk.
//!   The handler then errors and the action proceeds, so the harness reads as
//!   wired while enforcing nothing. Scoped to the manifest's artifacts because
//!   an anchored path the project *builds* — a bundler output, an installed
//!   binary — is legitimately absent before that build runs.
//! - **Managed-region edit** — content inside a `harnex-managed`
//!   sentinel block that diverges from the plugin's template.
//!
//! Spec-vocabulary staleness is deliberately NOT a finding here. It is a
//! property of the binary rather than of the project under audit, so it rides
//! the envelope's `warnings[]` on every command ([`crate::spec`]) instead of
//! appearing as a defect in one project's report — where it would also make a
//! fixture's zero-findings assertion fail on a calendar with no code change.
//!
//! Sub-auditors dispatch through [`AuditCheckKind`] — a closed-set
//! discriminator enum that drives `ProjectAuditor::run`'s exhaustive
//! match. Adding a variant forces every consuming site to update at
//! compile time; there is no parallel `KNOWN_*` const.
//!
//! ## What this module refuses to do
//!
//! - Never read rule / commit BODY prose for enforcement intent — that
//!   is a heuristic with a known false-positive floor. Audit findings
//!   are deterministic value / structural checks.
//! - Never modify any file. Findings only.
//! - Never spawn subprocesses.
//! - Never silently succeed when a configured sub-auditor's inputs are
//!   missing or malformed — return a typed error so a wrong invocation
//!   cannot masquerade as a clean audit.
//!
//! ## When to use vs `check`
//!
//! - `check` runs validators that the project configures
//!   (rules / skills / settings shape, codegen drift, permission auditor).
//! - `audit` runs harness-engineering checks — spec drift, managed-region
//!   integrity. Operators add `audit` to CI when they want enforcement
//!   beyond structural validation.

mod fill_marker;
mod hook_wiring;
mod managed_region;
mod settings_drift;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::envelope::{Finding, SkippedRule};
use crate::error::Result;
use crate::scaffold::{Artifact, Content, ScaffoldManifest};

use fill_marker::FillMarkerAuditor;
use hook_wiring::HookWiringAuditor;
use managed_region::ManagedRegionAuditor;
use settings_drift::SettingsDriftAuditor;

/// Closed set of audit checks the `harness audit` command dispatches.
/// `AuditCheckKind::ALL` drives [`ProjectAuditor::run`]'s exhaustive match
/// — adding a variant requires updating the `from_str`, `as_str`, and the
/// match arm in `run`, all enforced by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditCheckKind {
    SettingsDrift,
    HookWiring,
    ManagedRegion,
    FillMarker,
}

impl AuditCheckKind {
    pub const ALL: &'static [Self] = &[
        Self::SettingsDrift,
        Self::HookWiring,
        Self::ManagedRegion,
        Self::FillMarker,
    ];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "settings-drift" => Self::SettingsDrift,
            "hook-wiring" => Self::HookWiring,
            "managed-region" => Self::ManagedRegion,
            "fill-marker" => Self::FillMarker,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SettingsDrift => "settings-drift",
            Self::HookWiring => "hook-wiring",
            Self::ManagedRegion => "managed-region",
            Self::FillMarker => "fill-marker",
        }
    }
}

/// Which scaffold artifacts a project already holds. Presence is a fact, not
/// a verdict: an absent destination may be a gap, or a guarantee the project
/// keeps somewhere this auditor cannot see — server-side secret scanning, a
/// pre-receive hook, managed settings. Deciding which needs the project's
/// context, so the comparison is the skill's and this is its ground truth.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct CoverageEntry {
    /// `foundation` — language-agnostic; `language` — needs a detected stack.
    pub tier: String,
    /// The destination as the manifest declares it, `{lang}` unresolved.
    pub destination: String,
    pub present: bool,
}

/// Aggregate result of running every applicable sub-auditor.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct AuditOutcome {
    /// Findings sorted by (severity, slug, path) for deterministic output.
    pub findings: Vec<Finding>,
    /// Names of sub-auditors that actually ran.
    pub run: Vec<String>,
    /// Sub-auditors that did not run, with the reason.
    pub skipped: Vec<SkippedRule>,
    /// Count of unique files inspected across all sub-auditors.
    pub files_scanned: usize,
    /// Scaffold-artifact presence, in manifest order. Empty without a plugin
    /// root, which is the only place the composition is declared.
    #[serde(default)]
    pub coverage: Vec<CoverageEntry>,
}

pub struct ProjectAuditor<'a> {
    working_dir: &'a Path,
    /// Optional path to the plugin root (containing `templates/scaffold.toml`).
    /// When supplied, the managed-region auditor compares scaffolded artifacts
    /// against the canonical templates and the coverage block is populated.
    plugin_root: Option<PathBuf>,
}

impl<'a> ProjectAuditor<'a> {
    pub fn new(working_dir: &'a Path) -> Self {
        Self {
            working_dir,
            plugin_root: None,
        }
    }

    pub fn with_plugin_root(mut self, root: PathBuf) -> Self {
        self.plugin_root = Some(root);
        self
    }

    pub fn run(&self) -> Result<AuditOutcome> {
        let mut findings: Vec<Finding> = Vec::new();
        let mut run: Vec<String> = Vec::new();
        let mut skipped: Vec<SkippedRule> = Vec::new();
        let mut files_scanned: usize = 0;

        // The manifest is the only statement of what a harness is supposed to
        // contain, so the checks that need that knowledge load it once here
        // and skip explicitly when it was not supplied.
        let manifest = match &self.plugin_root {
            Some(root) => Some(ScaffoldManifest::load(&root.join("templates"))?),
            None => None,
        };
        let declared_artifacts: BTreeSet<String> = manifest
            .iter()
            .flat_map(|m| m.artifacts())
            .filter(|a| !a.destination_is_language_parameterized())
            .filter_map(|a| a.destination_for(None))
            .map(|d| d.to_string_lossy().to_string())
            .collect();

        // Drive dispatch through AuditCheckKind::ALL — the exhaustive match
        // below forces every variant to declare its wiring at compile time.
        for kind in AuditCheckKind::ALL {
            match kind {
                AuditCheckKind::SettingsDrift => {
                    let settings_path = self.working_dir.join(".claude/settings.json");
                    if settings_path.is_file() {
                        findings.extend(SettingsDriftAuditor::new().audit_file(&settings_path)?);
                        files_scanned += 1;
                        run.push(kind.as_str().to_string());
                    } else {
                        skipped.push(SkippedRule {
                            slug: kind.as_str().to_string(),
                            reason: ".claude/settings.json not present".into(),
                        });
                    }
                }
                AuditCheckKind::HookWiring => {
                    let settings_path = self.working_dir.join(".claude/settings.json");
                    let reason = if manifest.is_none() {
                        Some("no plugin root supplied (use --plugin-root)")
                    } else if !settings_path.is_file() {
                        Some(".claude/settings.json not present")
                    } else {
                        None
                    };
                    match reason {
                        Some(reason) => skipped.push(SkippedRule {
                            slug: kind.as_str().to_string(),
                            reason: reason.into(),
                        }),
                        None => {
                            findings.extend(
                                HookWiringAuditor::new(&declared_artifacts)
                                    .audit_file(&settings_path, self.working_dir)?,
                            );
                            files_scanned += 1;
                            run.push(kind.as_str().to_string());
                        }
                    }
                }
                AuditCheckKind::ManagedRegion => {
                    let Some(plugin_root) = self.plugin_root.as_ref() else {
                        skipped.push(SkippedRule {
                            slug: kind.as_str().to_string(),
                            reason: "no plugin root supplied (use --plugin-root)".into(),
                        });
                        continue;
                    };
                    let outcome = ManagedRegionAuditor::new(plugin_root).audit(self.working_dir)?;
                    files_scanned += outcome.files_scanned;
                    findings.extend(outcome.findings);
                    run.push(kind.as_str().to_string());
                }
                AuditCheckKind::FillMarker => {
                    let outcome = FillMarkerAuditor::new().audit(self.working_dir)?;
                    files_scanned += outcome.files_scanned;
                    findings.extend(outcome.findings);
                    run.push(kind.as_str().to_string());
                }
            }
        }

        findings.sort_by(|a, b| {
            a.severity
                .rank()
                .cmp(&b.severity.rank())
                .then(a.slug.cmp(&b.slug))
                .then(a.location.path.as_path().cmp(b.location.path.as_path()))
        });
        run.sort();
        skipped.sort_by(|a, b| a.slug.cmp(&b.slug));

        Ok(AuditOutcome {
            findings,
            run,
            skipped,
            files_scanned,
            coverage: self.coverage()?,
        })
    }

    /// Presence of every scaffold artifact, in manifest order. A destination
    /// the manifest parameterizes by language has one name per shipped
    /// language, and holding any of them is coverage — so the answer needs no
    /// stack detection and every path tested is one the scaffold would emit.
    ///
    /// Exact paths rather than a pattern, on both counts that matter: a `*`
    /// standing in for `{lang}` counts a project's own `api-conventions.md` as
    /// coverage, and a project path carrying `[` or `?` turns the search
    /// itself into a pattern over a tree nobody named.
    fn coverage(&self) -> Result<Vec<CoverageEntry>> {
        let Some(plugin_root) = &self.plugin_root else {
            return Ok(Vec::new());
        };
        let manifest = ScaffoldManifest::load(&plugin_root.join("templates"))?;
        let templates = plugin_root.join("templates");
        let mut entries = Vec::new();
        for artifact in manifest.artifacts() {
            let present = match &artifact.content {
                // Several artifacts share one destination, so the file
                // existing says nothing about which of them contributed. The
                // fragment landing at its key path is the only exact answer —
                // a foundation-only scaffold otherwise reports its unmerged
                // language rows present because the foundation wrote the file.
                Content::Merge { key } => self.fragment_landed(&templates, artifact, key),
                Content::Copy | Content::Seed | Content::Managed => artifact
                    .resolved_destinations()
                    .iter()
                    .any(|d| self.working_dir.join(d).exists()),
            };
            entries.push(CoverageEntry {
                tier: artifact.tier.as_str().to_string(),
                destination: artifact.destination.clone(),
                present,
            });
        }
        Ok(entries)
    }

    /// Whether this fragment's own contribution is in the destination.
    ///
    /// Containment rather than equality: the destination is shared, and an
    /// operator's own entries beside harnex's are the documented arrangement,
    /// not drift. Unreadable or unparseable either side answers "not landed" —
    /// coverage is a fact about what is there, and nothing here is a finding.
    fn fragment_landed(&self, templates: &Path, artifact: &Artifact, key: &str) -> bool {
        artifact.resolved_templates().iter().any(|template| {
            let Ok(raw) = std::fs::read_to_string(templates.join(template)) else {
                return false;
            };
            let Ok(fragment) = serde_json::from_str::<serde_json::Value>(&raw) else {
                return false;
            };
            artifact.resolved_destinations().iter().any(|d| {
                std::fs::read_to_string(self.working_dir.join(d))
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .and_then(|doc| value_at(&doc, key).cloned())
                    .is_some_and(|landed| contains_value(&landed, &fragment))
            })
        })
    }
}

/// The value at a dotted key path, or `None` if any segment is absent.
fn value_at<'a>(doc: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    key.split('.').try_fold(doc, |node, seg| node.get(seg))
}

/// Whether `whole` carries everything `part` declares.
///
/// Objects match key-wise so a shared map holds every contributor; arrays
/// match element-wise and unordered, because a merge appends and the order two
/// fragments land in is not a property either of them declares.
fn contains_value(whole: &serde_json::Value, part: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (whole, part) {
        (Value::Object(w), Value::Object(p)) => p
            .iter()
            .all(|(k, v)| w.get(k).is_some_and(|got| contains_value(got, v))),
        (Value::Array(w), Value::Array(p)) => {
            p.iter().all(|v| w.iter().any(|got| contains_value(got, v)))
        }
        _ => whole == part,
    }
}

#[cfg(test)]
mod containment_tests {
    use super::{contains_value, value_at};
    use serde_json::json;

    #[test]
    fn a_fragment_is_found_beside_another_contributors_entries() {
        // Two tiers merge into `hooks`, and an operator's own entries sit
        // beside both. Containment is the question; equality would report the
        // foundation's contribution missing the moment a language tier landed.
        let settings = json!({"hooks": {
            "SessionStart": [{"matcher": "startup"}],
            "Stop": [{}],
            "PostToolUse": [{"matcher": "Edit|Write"}],
            "PreToolUse": [{"matcher": "operator's own"}],
        }});
        let foundation = json!({"SessionStart": [{"matcher": "startup"}], "Stop": [{}]});
        let language = json!({"PostToolUse": [{"matcher": "Edit|Write"}]});
        let landed = value_at(&settings, "hooks").unwrap();
        assert!(contains_value(landed, &foundation));
        assert!(contains_value(landed, &language));
    }

    #[test]
    fn a_fragment_that_never_merged_is_not_found() {
        // The foundation-only case. The destination exists because the
        // foundation wrote it, which is exactly why its existence cannot
        // answer for the language tier.
        let settings = json!({"hooks": {"SessionStart": [{"matcher": "startup"}]}});
        let language = json!({"PostToolUse": [{"matcher": "Edit|Write"}]});
        let landed = value_at(&settings, "hooks").unwrap();
        assert!(!contains_value(landed, &language));
    }

    #[test]
    fn an_array_matches_element_wise_and_unordered() {
        // A merge appends, and the order two fragments land in is a property
        // neither of them declares.
        let whole = json!(["Bash(git commit *)", "Read", "Bash(uv *)"]);
        assert!(contains_value(&whole, &json!(["Bash(uv *)", "Read"])));
        assert!(!contains_value(&whole, &json!(["Bash(poe *)"])));
    }

    #[test]
    fn an_absent_key_path_is_absent_rather_than_empty() {
        let doc = json!({"permissions": {"deny": []}});
        assert!(value_at(&doc, "permissions.deny").is_some());
        assert!(value_at(&doc, "permissions.allow").is_none());
        assert!(value_at(&doc, "hooks.SessionStart").is_none());
    }
}

#[cfg(test)]
mod kind_tests {
    use super::AuditCheckKind;

    #[test]
    fn from_str_round_trips_every_variant() {
        for k in AuditCheckKind::ALL {
            assert_eq!(AuditCheckKind::from_str(k.as_str()), Some(*k));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert!(AuditCheckKind::from_str("made-up").is_none());
    }
}
