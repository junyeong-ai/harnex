//! # audit — harness-engineering compliance gate
//!
//! Distinct from [`check`](crate::check), which validates structural
//! correctness, `audit` evaluates *engineering quality* of the generated
//! harness against spec-facts and the keep-soften-cut policy.
//!
//! Every check is a variant of [`AuditCheckKind`], which carries what that
//! check asks and why. Restating the set here is what left this doc claiming
//! three classes while five ran, so the enum is the only list.
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
//! - `audit` holds the generated harness to the composition it was generated
//!   from, and to the live Claude Code spec. Operators add `audit` to CI when
//!   they want enforcement beyond structural validation.

mod copy_drift;
mod fill_marker;
mod hook_wiring;
mod managed_region;
mod settings_drift;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::envelope::{Finding, SkippedRule};
use crate::error::Result;
use crate::scaffold::{self, Artifact, Content, ScaffoldManifest};

use copy_drift::CopyDriftAuditor;
use fill_marker::FillMarkerAuditor;
use hook_wiring::HookWiringAuditor;
use managed_region::ManagedRegionAuditor;
use settings_drift::SettingsDriftAuditor;

/// Normalize generated content for comparison: collapse CRLF, trim the edges.
///
/// One owner because two auditors compare a project's copy of a template
/// against that template, and a line ending is not a difference either of them
/// means. A Windows checkout with `core.autocrlf` on, or a `.gitattributes`
/// that normalizes, otherwise reports every shell hook as drifted with no fix
/// short of committing different bytes than the template.
pub(crate) fn normalize(body: &str) -> String {
    body.replace("\r\n", "\n").trim().to_string()
}

/// Closed set of audit checks the `harness audit` command dispatches, and the
/// only statement of what an audit covers — the module doc points here rather
/// than repeating it, because a restated list is what drifts.
///
/// `AuditCheckKind::ALL` drives [`ProjectAuditor::run`]'s exhaustive match —
/// adding a variant requires updating the `from_str`, `as_str`, and the match
/// arm in `run`, all enforced by the compiler. Document the new variant here
/// in the same edit: this is the doc every other surface defers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditCheckKind {
    /// Values in `.claude/settings.json` that look plausible and violate the
    /// live Claude Code spec — a millisecond `timeout`, an `mcp__server`
    /// matcher missing the suffix that makes it match anything.
    SettingsDrift,
    /// A hook naming a scaffold artifact that is not on disk. The handler then
    /// errors and the action proceeds, so the harness reads as wired while
    /// enforcing nothing. Scoped to the manifest's artifacts because an
    /// anchored path the project *builds* — a bundler output, an installed
    /// binary — is legitimately absent before that build runs.
    HookWiring,
    /// Content inside a `harnex-managed` sentinel block that diverges from the
    /// plugin's template, and a managed artifact whose sentinels are gone —
    /// regenerate then has nothing to write into.
    ManagedRegion,
    /// A `copy` artifact whose bytes differ from the template that emits it.
    /// The manifest calls that a defect by definition, and it is how a
    /// project's own file ends up at a destination the hook fragments wire
    /// into: ownership is decided per artifact, the wiring lives in another
    /// one.
    CopyDrift,
    /// A `harnex-fill` marker the generating step left behind, over `CLAUDE.md`
    /// and `.claude/**/*.md`. A placeholder that ships is the blank page the
    /// templates exist to avoid, arriving as a finished-looking file.
    FillMarker,
}

impl AuditCheckKind {
    pub const ALL: &'static [Self] = &[
        Self::SettingsDrift,
        Self::HookWiring,
        Self::ManagedRegion,
        Self::CopyDrift,
        Self::FillMarker,
    ];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "settings-drift" => Self::SettingsDrift,
            "hook-wiring" => Self::HookWiring,
            "managed-region" => Self::ManagedRegion,
            "copy-drift" => Self::CopyDrift,
            "fill-marker" => Self::FillMarker,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SettingsDrift => "settings-drift",
            Self::HookWiring => "hook-wiring",
            Self::ManagedRegion => "managed-region",
            Self::CopyDrift => "copy-drift",
            Self::FillMarker => "fill-marker",
        }
    }
}

/// Closed set of slugs an audit finding can carry.
///
/// A slug is a wire contract — CI greps it, the plugin's audit mode explains
/// it to an operator — so it is a vocabulary, not a string literal at the emit
/// site. The literals had no owner, and the check added most recently reached
/// two shipped documents in neither: an operator saw a finding the skill could
/// not name. `audit_slug_sync` holds this list against those documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditFindingSlug {
    MsTimeout,
    McpMatcherIncomplete,
    HookScriptMissing,
    HookNotExecutable,
    ManagedRegionEdited,
    ManagedRegionMissing,
    CopyDrift,
    FillMarkerUnresolved,
}

impl AuditFindingSlug {
    pub const ALL: &'static [Self] = &[
        Self::MsTimeout,
        Self::McpMatcherIncomplete,
        Self::HookScriptMissing,
        Self::HookNotExecutable,
        Self::ManagedRegionEdited,
        Self::ManagedRegionMissing,
        Self::CopyDrift,
        Self::FillMarkerUnresolved,
    ];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "audit-ms-timeout" => Self::MsTimeout,
            "audit-mcp-matcher-incomplete" => Self::McpMatcherIncomplete,
            "audit-hook-script-missing" => Self::HookScriptMissing,
            "audit-hook-not-executable" => Self::HookNotExecutable,
            "audit-managed-region-edited" => Self::ManagedRegionEdited,
            "audit-managed-region-missing" => Self::ManagedRegionMissing,
            "audit-copy-drift" => Self::CopyDrift,
            "audit-fill-marker-unresolved" => Self::FillMarkerUnresolved,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MsTimeout => "audit-ms-timeout",
            Self::McpMatcherIncomplete => "audit-mcp-matcher-incomplete",
            Self::HookScriptMissing => "audit-hook-script-missing",
            Self::HookNotExecutable => "audit-hook-not-executable",
            Self::ManagedRegionEdited => "audit-managed-region-edited",
            Self::ManagedRegionMissing => "audit-managed-region-missing",
            Self::CopyDrift => "audit-copy-drift",
            Self::FillMarkerUnresolved => "audit-fill-marker-unresolved",
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
    /// The key a `merge` fragment contributes at; `None` for every other kind.
    ///
    /// Five artifacts land in `.claude/settings.json` and the destination is
    /// the same string for all of them, so without the key the block is rows
    /// that differ only in `tier` and a reader cannot tell which contribution
    /// is the one reporting absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contributes: Option<String>,
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
        // Every destination the manifest can name, one concrete path per
        // language. Skipping the `{lang}` artifacts would exempt the formatter
        // hook — the one language-tier script a hook entry points at — from the
        // check that exists to catch a hook wired at a file that is not there.
        let declared_artifacts: BTreeSet<String> = manifest
            .iter()
            .flat_map(|m| m.artifacts())
            .flat_map(Artifact::resolved_destinations)
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
                AuditCheckKind::CopyDrift => {
                    let Some(plugin_root) = self.plugin_root.as_ref() else {
                        skipped.push(SkippedRule {
                            slug: kind.as_str().to_string(),
                            reason: "no plugin root supplied (use --plugin-root)".into(),
                        });
                        continue;
                    };
                    findings.extend(CopyDriftAuditor.audit(self.working_dir, plugin_root)?);
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
                contributes: match &artifact.content {
                    Content::Merge { key } => Some(key.clone()),
                    _ => None,
                },
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
                    .is_some_and(|doc| scaffold::fragment_landed(&doc, key, &fragment))
            })
        })
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
