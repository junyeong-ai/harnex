//! # check — unified validation gate
//!
//! Runs every enabled validator (rules, skills, settings, evidence,
//! codegen, permission audit) over the configured surfaces and emits
//! a single aggregated `CheckOutcome` envelope. Each finding's `slug`
//! attributes it to the producing validator.
//!
//! Supports `--since <git-ref>` to restrict scanning to files changed
//! since the ref — same semantics as nodex's `check --since`. Without
//! `--since`, every discovered candidate is scanned.
//!
//! ## What this module refuses to do
//!
//! - Never run a validator whose config section is absent — it surfaces
//!   in `skipped` instead, so the consumer knows the absence is explicit.
//! - Never mutate any file. The check is read-only.
//! - Never spawn subprocesses except `git diff --name-only <ref>` when
//!   `--since` is used.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::codegen::SentinelSyncer;
use crate::config::Config;
use crate::envelope::{Finding, Location, Severity, SkippedRule};
use crate::error::{Error, Result};
use crate::evidence::EvidenceVerifier;
use crate::policy::{PermissionAuditor, PermissionFindingKind};
use crate::validate::{RuleValidator, SettingsScope, SettingsValidator, SkillValidator};

/// Aggregate result of running every enabled validator.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct CheckOutcome {
    /// Findings sorted by (severity, slug, path) for deterministic output.
    pub findings: Vec<Finding>,
    /// Slugs of validators that actually ran.
    pub run: Vec<String>,
    /// Validators that were not run, with the reason.
    pub skipped: Vec<SkippedRule>,
    /// Count of unique files scanned across all validators.
    pub files_scanned: usize,
}

/// Result of `ProjectChecker::fix` — before-check snapshot, fix attempts,
/// after-check snapshot. Consumers compare `before.findings.len()` vs
/// `after.findings.len()` to confirm convergence.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct FixOutcome {
    pub before: CheckOutcome,
    pub fixes_attempted: Vec<FixAttempt>,
    pub after: CheckOutcome,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct FixAttempt {
    pub fix_command: String,
    /// Slugs of findings this fix targeted.
    pub finding_slugs: Vec<String>,
    pub status: FixStatus,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum FixStatus {
    /// Fix function ran successfully.
    Applied,
    /// Fix function ran but returned an error.
    Failed { reason: String },
    /// Fix command is not in the safe-fix registry; never executed.
    Unrecognized,
}

/// Closed set of auto-fix commands the safe-fix registry recognises.
///
/// Single source of truth for both validator emit sites (which must
/// produce `fix_command: Some(FixCommand::X.as_str().into())`) and
/// [`ProjectChecker::try_fix`] (which dispatches via exhaustive `match`
/// on this enum). Adding a new variant forces both sites to update at
/// compile time — there is no drift class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixCommand {
    CodegenSync,
}

impl FixCommand {
    pub const ALL: &'static [Self] = &[Self::CodegenSync];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "harness codegen sync" => Self::CodegenSync,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodegenSync => "harness codegen sync",
        }
    }
}

pub struct ProjectChecker<'a> {
    config: &'a Config,
    working_dir: &'a Path,
    since: Option<&'a str>,
}

impl<'a> ProjectChecker<'a> {
    pub fn new(config: &'a Config, working_dir: &'a Path) -> Self {
        Self {
            config,
            working_dir,
            since: None,
        }
    }

    pub fn with_since(mut self, since: &'a str) -> Self {
        self.since = Some(since);
        self
    }

    /// Run check, execute every auto_fixable finding via the safe-fix
    /// registry, then re-run check. When no auto_fixable findings exist
    /// the second run is skipped (`before == after`).
    pub fn fix(&self) -> Result<FixOutcome> {
        use std::collections::BTreeMap;

        let before = self.run()?;
        let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for f in &before.findings {
            if f.auto_fixable
                && let Some(cmd) = &f.fix_command
            {
                grouped.entry(cmd.clone()).or_default().push(f.slug.clone());
            }
        }
        if grouped.is_empty() {
            let after = before.clone();
            return Ok(FixOutcome {
                before,
                fixes_attempted: Vec::new(),
                after,
            });
        }
        let mut attempts: Vec<FixAttempt> = grouped
            .into_iter()
            .map(|(cmd, slugs)| {
                let status = self.try_fix(&cmd);
                FixAttempt {
                    fix_command: cmd,
                    finding_slugs: slugs,
                    status,
                }
            })
            .collect();
        attempts.sort_by(|a, b| a.fix_command.cmp(&b.fix_command));
        let after = self.run()?;
        Ok(FixOutcome {
            before,
            fixes_attempted: attempts,
            after,
        })
    }

    /// Safe-fix registry. Dispatches on the [`FixCommand`] enum — the
    /// single source of truth for both validator emit sites and the
    /// match below. Adding a new auto-fixable finding requires:
    /// 1. Add a [`FixCommand`] variant + its `as_str()` mapping.
    /// 2. Emit findings with `fix_command: Some(FixCommand::X.as_str().into())`.
    /// 3. Add a match arm here (the compiler enforces exhaustiveness on
    ///    `FixCommand`, so missing this step is a build error).
    /// 4. Add a test asserting drift → fix → 0 findings.
    fn try_fix(&self, cmd: &str) -> FixStatus {
        let Some(parsed) = FixCommand::from_str(cmd) else {
            return FixStatus::Unrecognized;
        };
        match parsed {
            FixCommand::CodegenSync => {
                let Some(cfg) = self.config.codegen.as_ref() else {
                    return FixStatus::Failed {
                        reason: "no [codegen] section in harness.toml".into(),
                    };
                };
                match SentinelSyncer::new(cfg, self.working_dir).sync() {
                    Ok(_) => FixStatus::Applied,
                    Err(e) => FixStatus::Failed {
                        reason: e.to_string(),
                    },
                }
            }
        }
    }

    pub fn run(&self) -> Result<CheckOutcome> {
        let mut findings: Vec<Finding> = Vec::new();
        let mut run: Vec<String> = Vec::new();
        let mut skipped: Vec<SkippedRule> = Vec::new();
        let mut files_scanned = 0usize;
        let changed = self.changed_files()?;

        self.run_rule_validator(
            &changed,
            &mut findings,
            &mut run,
            &mut skipped,
            &mut files_scanned,
        )?;
        self.run_skill_validator(
            &changed,
            &mut findings,
            &mut run,
            &mut skipped,
            &mut files_scanned,
        )?;
        self.run_settings_validator(
            &changed,
            &mut findings,
            &mut run,
            &mut skipped,
            &mut files_scanned,
        )?;
        self.run_evidence(
            &changed,
            &mut findings,
            &mut run,
            &mut skipped,
            &mut files_scanned,
        )?;
        self.run_codegen(&mut findings, &mut run, &mut skipped)?;
        self.run_permissions_audit(&changed, &mut findings, &mut run, &mut skipped)?;

        findings.sort_by(|a, b| {
            a.severity
                .rank()
                .cmp(&b.severity.rank())
                .then(a.slug.cmp(&b.slug))
                .then(a.location.path.as_path().cmp(b.location.path.as_path()))
        });
        run.sort();
        skipped.sort_by(|a, b| a.slug.cmp(&b.slug));

        Ok(CheckOutcome {
            findings,
            run,
            skipped,
            files_scanned,
        })
    }

    fn changed_files(&self) -> Result<Option<HashSet<PathBuf>>> {
        let Some(since) = self.since else {
            return Ok(None);
        };
        let output = Command::new("git")
            .args(["diff", "--name-only", since])
            .current_dir(self.working_dir)
            .output()
            .map_err(|e| Error::CheckGitFailure {
                message: format!("git diff --name-only {since} spawn: {e}"),
            })?;
        if !output.status.success() {
            return Err(Error::CheckGitFailure {
                message: format!(
                    "git diff --name-only {since} failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        let raw = String::from_utf8_lossy(&output.stdout);
        let set: HashSet<PathBuf> = raw.lines().map(|l| self.working_dir.join(l)).collect();
        Ok(Some(set))
    }

    fn passes_filter(&self, path: &Path, changed: &Option<HashSet<PathBuf>>) -> bool {
        match changed {
            Some(set) => set.contains(path),
            None => true,
        }
    }

    fn discover_glob(&self, pattern: &str) -> Vec<PathBuf> {
        let full = self.working_dir.join(pattern);
        let Some(s) = full.to_str() else {
            return Vec::new();
        };
        glob::glob(s)
            .map(|iter| iter.filter_map(std::result::Result::ok).collect())
            .unwrap_or_default()
    }

    fn run_rule_validator(
        &self,
        changed: &Option<HashSet<PathBuf>>,
        findings: &mut Vec<Finding>,
        run: &mut Vec<String>,
        skipped: &mut Vec<SkippedRule>,
        files_scanned: &mut usize,
    ) -> Result<()> {
        let Some(policy) = self.config.validate.as_ref().and_then(|v| v.rules.as_ref()) else {
            skipped.push(SkippedRule {
                slug: "validate.rules".into(),
                reason: "no [validate.rules] section".into(),
            });
            return Ok(());
        };
        let validator = RuleValidator::new(policy);
        let candidates = self.discover_glob(".claude/rules/*.md");
        for path in &candidates {
            if !self.passes_filter(path, changed) {
                continue;
            }
            *files_scanned += 1;
            findings.extend(validator.validate_file(path)?);
        }
        run.push("validate.rules".into());
        Ok(())
    }

    fn run_skill_validator(
        &self,
        changed: &Option<HashSet<PathBuf>>,
        findings: &mut Vec<Finding>,
        run: &mut Vec<String>,
        skipped: &mut Vec<SkippedRule>,
        files_scanned: &mut usize,
    ) -> Result<()> {
        let Some(policy) = self
            .config
            .validate
            .as_ref()
            .and_then(|v| v.skills.as_ref())
        else {
            skipped.push(SkippedRule {
                slug: "validate.skills".into(),
                reason: "no [validate.skills] section".into(),
            });
            return Ok(());
        };
        let validator = SkillValidator::new(policy);
        let candidates = self.discover_glob(".claude/skills/*/SKILL.md");
        for path in &candidates {
            if !self.passes_filter(path, changed) {
                continue;
            }
            *files_scanned += 1;
            findings.extend(validator.validate_file(path)?);
        }
        run.push("validate.skills".into());
        Ok(())
    }

    fn run_settings_validator(
        &self,
        changed: &Option<HashSet<PathBuf>>,
        findings: &mut Vec<Finding>,
        run: &mut Vec<String>,
        skipped: &mut Vec<SkippedRule>,
        files_scanned: &mut usize,
    ) -> Result<()> {
        let path = self.working_dir.join(".claude/settings.json");
        if !path.is_file() {
            skipped.push(SkippedRule {
                slug: "validate.settings".into(),
                reason: ".claude/settings.json not present".into(),
            });
            return Ok(());
        }
        if !self.passes_filter(&path, changed) {
            run.push("validate.settings".into());
            return Ok(());
        }
        // ProjectChecker runs against a project root, so the scope is
        // unambiguous: project. The local override (`settings.local.json`) is
        // discovered separately if present.
        findings.extend(SettingsValidator::new().validate_file(&path, SettingsScope::Project)?);
        *files_scanned += 1;
        let local = self.working_dir.join(".claude/settings.local.json");
        if local.is_file() && self.passes_filter(&local, changed) {
            findings.extend(SettingsValidator::new().validate_file(&local, SettingsScope::Local)?);
            *files_scanned += 1;
        }
        run.push("validate.settings".into());
        Ok(())
    }

    fn run_evidence(
        &self,
        changed: &Option<HashSet<PathBuf>>,
        findings: &mut Vec<Finding>,
        run: &mut Vec<String>,
        skipped: &mut Vec<SkippedRule>,
        files_scanned: &mut usize,
    ) -> Result<()> {
        let Some(cfg) = self.config.evidence.as_ref() else {
            skipped.push(SkippedRule {
                slug: "evidence".into(),
                reason: "no [evidence] section".into(),
            });
            return Ok(());
        };
        let verifier = EvidenceVerifier::new(cfg)?;
        let mut candidates: Vec<PathBuf> = vec![self.working_dir.join("CLAUDE.md")];
        candidates.extend(self.discover_glob(".claude/rules/*.md"));
        for path in &candidates {
            if !path.is_file() {
                continue;
            }
            if !self.passes_filter(path, changed) {
                continue;
            }
            *files_scanned += 1;
            findings.extend(verifier.verify_file(path, self.working_dir)?);
        }
        run.push("evidence".into());
        Ok(())
    }

    fn run_codegen(
        &self,
        findings: &mut Vec<Finding>,
        run: &mut Vec<String>,
        skipped: &mut Vec<SkippedRule>,
    ) -> Result<()> {
        let Some(cfg) = self.config.codegen.as_ref() else {
            skipped.push(SkippedRule {
                slug: "codegen".into(),
                reason: "no [codegen] section".into(),
            });
            return Ok(());
        };
        let outcomes = SentinelSyncer::new(cfg, self.working_dir).check()?;
        for o in &outcomes {
            if o.changed {
                findings.push(Finding {
                    slug: "codegen-drift".into(),
                    severity: Severity::Blocker,
                    location: Location::file(o.target.clone()),
                    message: format!("group '{}': target drifts from source", o.group),
                    hint: Some(format!(
                        "run `{}` to regenerate",
                        FixCommand::CodegenSync.as_str()
                    )),
                    auto_fixable: true,
                    fix_command: Some(FixCommand::CodegenSync.as_str().into()),
                });
            }
        }
        run.push("codegen".into());
        Ok(())
    }

    fn run_permissions_audit(
        &self,
        changed: &Option<HashSet<PathBuf>>,
        findings: &mut Vec<Finding>,
        run: &mut Vec<String>,
        skipped: &mut Vec<SkippedRule>,
    ) -> Result<()> {
        let Some(policy_cfg) = self.config.policy.as_ref() else {
            skipped.push(SkippedRule {
                slug: "policy.permissions".into(),
                reason: "no [policy] section".into(),
            });
            return Ok(());
        };
        let Some(perms_policy) = policy_cfg.permissions.as_ref() else {
            skipped.push(SkippedRule {
                slug: "policy.permissions".into(),
                reason: "no [policy.permissions] section".into(),
            });
            return Ok(());
        };
        let settings_path = self.working_dir.join(".claude/settings.json");
        if !settings_path.is_file() {
            skipped.push(SkippedRule {
                slug: "policy.permissions".into(),
                reason: ".claude/settings.json not present".into(),
            });
            return Ok(());
        }
        if !self.passes_filter(&settings_path, changed) {
            run.push("policy.permissions".into());
            return Ok(());
        }
        let raw = std::fs::read_to_string(&settings_path).map_err(|e| Error::IoFailure {
            path: settings_path.clone(),
            source: e,
        })?;
        let v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| Error::ConfigInvalid {
                message: format!("settings.json parse: {e}"),
                location: None,
            })?;
        let allow: Vec<String> = v
            .pointer("/permissions/allow")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let ask: Vec<String> = v
            .pointer("/permissions/ask")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let deny: Vec<String> = v
            .pointer("/permissions/deny")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let perm_findings = PermissionAuditor::new(perms_policy, &allow, &ask, &deny).audit();
        for pf in &perm_findings {
            findings.push(Finding {
                slug: match pf.kind {
                    PermissionFindingKind::MissingBaselineDeny => {
                        "permission-missing-baseline-deny".into()
                    }
                    PermissionFindingKind::ContradictoryRule => "permission-contradictory-rule".into(),
                },
                severity: Severity::Major,
                location: Location::file(settings_path.clone()),
                message: pf.message.clone(),
                hint: Some(format!(
                    "regenerate from canonical profiles: `harness policy permissions generate {} > .claude/settings.json`",
                    self.config
                        .policy
                        .as_ref()
                        .and_then(|p| p.permissions.as_ref())
                        .map(|p| p
                            .profiles
                            .iter()
                            .map(|s| format!("--profile {s}"))
                            .collect::<Vec<_>>()
                            .join(" "))
                        .unwrap_or_else(|| "--profile baseline".into())
                )),
                auto_fixable: false,
                fix_command: pf
                    .rule
                    .as_ref()
                    .map(|r| format!("# add to permissions.deny: {r}")),
            });
        }
        run.push("policy.permissions".into());
        Ok(())
    }
}

#[cfg(test)]
mod fix_command_tests {
    use super::FixCommand;

    #[test]
    fn from_str_round_trips_every_variant() {
        for c in FixCommand::ALL {
            assert_eq!(FixCommand::from_str(c.as_str()), Some(*c));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert_eq!(FixCommand::from_str("rm -rf /"), None);
    }
}
