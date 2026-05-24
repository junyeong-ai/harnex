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
//!   matcher, Stop hook with a non-zero exit path).
//! - **Managed-region edit** — content inside a `harnex-managed`
//!   sentinel block that diverges from the plugin's template.
//! - **Reserved for future enforced-vs-advisory misalignment checks.**
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

mod managed_region;
mod settings_drift;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::envelope::{Finding, SkippedRule};
use crate::error::Result;

use managed_region::ManagedRegionAuditor;
use settings_drift::SettingsDriftAuditor;

/// Closed set of audit checks the `harness audit` command dispatches.
/// `AuditCheckKind::ALL` drives [`ProjectAuditor::run`]'s exhaustive match
/// — adding a variant requires updating the `from_str`, `as_str`, and the
/// match arm in `run`, all enforced by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditCheckKind {
    SettingsDrift,
    ManagedRegion,
}

impl AuditCheckKind {
    pub const ALL: &'static [Self] = &[Self::SettingsDrift, Self::ManagedRegion];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "settings-drift" => Self::SettingsDrift,
            "managed-region" => Self::ManagedRegion,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SettingsDrift => "settings-drift",
            Self::ManagedRegion => "managed-region",
        }
    }
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
}

pub struct ProjectAuditor<'a> {
    working_dir: &'a Path,
    /// Optional path to the plugin root (containing `templates/managed-files.toml`).
    /// When supplied, the managed-region auditor compares scaffolded
    /// artifacts against the canonical templates.
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
                AuditCheckKind::ManagedRegion => {
                    let Some(plugin_root) = &self.plugin_root else {
                        skipped.push(SkippedRule {
                            slug: kind.as_str().to_string(),
                            reason: "no plugin root supplied (use --plugin-root)".into(),
                        });
                        continue;
                    };
                    let outcome =
                        ManagedRegionAuditor::new(plugin_root).audit(self.working_dir)?;
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
