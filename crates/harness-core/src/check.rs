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
//! - Never spawn a subprocess but git, and git only to ask which files are
//!   the project's: `git diff --name-only -z --relative <ref>` under `--since`,
//!   and `git ls-files` for the nested `CLAUDE.md` set. The first failing is
//!   an error, because the caller asked for a git-derived window; the second
//!   failing declares that set unmeasured in `skipped` and reads the rest,
//!   because a tarball export or a container with dubious ownership still
//!   has a root memory file and rules to check.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::codegen::SentinelSyncer;
use crate::config::Config;
use crate::envelope::{Finding, FixCommand, Location, Severity, SkippedRule};
use crate::error::{Error, Result};
use crate::evidence::EvidenceVerifier;
use crate::policy::{PermissionAuditor, PermissionFindingKind};
use crate::validate::{
    AgentValidator, OutputStyleValidator, RoutineValidator, RuleValidator, SettingsScope,
    SettingsValidator, SkillValidator, SurfaceValidator,
};

/// Aggregate result of running every enabled validator.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct CheckOutcome {
    /// Findings sorted by (severity, slug, path) for deterministic output.
    pub findings: Vec<Finding>,
    /// Slugs of validators that actually ran.
    pub run: Vec<String>,
    /// Validators that were not run, with the reason.
    pub skipped: Vec<SkippedRule>,
    /// Count of scans across the windowed validators — a file more than one
    /// validator reads counts once per reader.
    pub files_scanned: usize,
}

/// Result of `ProjectChecker::fix` — before-check snapshot, fix attempts,
/// after-check snapshot. Consumers compare `before.findings.len()` vs
/// `after.findings.len()` to confirm convergence.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct FixReport {
    pub before: CheckOutcome,
    pub fixes_attempted: Vec<FixAttempt>,
    pub after: CheckOutcome,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct FixAttempt {
    pub fix_command: FixCommand,
    /// Slugs of findings this fix targeted.
    pub finding_slugs: Vec<String>,
    pub outcome: FixOutcome,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum FixOutcome {
    /// Fix function ran successfully.
    Applied,
    /// Fix function ran but returned an error.
    Failed { reason: String },
}

pub struct ProjectChecker<'a> {
    config: &'a Config,
    working_dir: &'a Path,
    since: Option<&'a str>,
    unattended: bool,
}

impl<'a> ProjectChecker<'a> {
    pub fn new(config: &'a Config, working_dir: &'a Path) -> Self {
        Self {
            config,
            working_dir,
            since: None,
            unattended: false,
        }
    }

    pub fn with_since(mut self, since: &'a str) -> Self {
        self.since = Some(since);
        self
    }

    /// Declare an unattended context — a push gate, CI. Advisory staleness
    /// then gates only where the entry declares the re-measurement clearable
    /// in the same sitting.
    pub fn with_unattended(mut self) -> Self {
        self.unattended = true;
        self
    }

    /// Run check, execute every auto_fixable finding via the safe-fix
    /// registry, then re-run check. When no auto_fixable findings exist
    /// the second run is skipped (`before == after`).
    pub fn fix(&self) -> Result<FixReport> {
        use std::collections::BTreeMap;

        let before = self.run()?;
        let mut grouped: BTreeMap<&'static str, (FixCommand, Vec<String>)> = BTreeMap::new();
        for f in &before.findings {
            if f.auto_fixable
                && let Some(cmd) = f.fix_command
            {
                grouped
                    .entry(cmd.as_str())
                    .or_insert_with(|| (cmd, Vec::new()))
                    .1
                    .push(f.slug.clone());
            }
        }
        if grouped.is_empty() {
            let after = before.clone();
            return Ok(FixReport {
                before,
                fixes_attempted: Vec::new(),
                after,
            });
        }
        let mut attempts: Vec<FixAttempt> = grouped
            .into_iter()
            .map(|(_, (cmd, slugs))| {
                let outcome = self.try_fix(cmd);
                FixAttempt {
                    fix_command: cmd,
                    finding_slugs: slugs,
                    outcome,
                }
            })
            .collect();
        attempts.sort_by_key(|a| a.fix_command.as_str());
        let after = self.run()?;
        Ok(FixReport {
            before,
            fixes_attempted: attempts,
            after,
        })
    }

    /// Safe-fix registry. Dispatches on the [`FixCommand`] enum — the
    /// single source of truth for both validator emit sites and the
    /// match below. Adding a new auto-fixable finding requires:
    /// 1. Add a [`FixCommand`] variant + its `as_str()` mapping.
    /// 2. Emit findings with `fix_command: Some(FixCommand::X)` — the field is
    ///    typed, so this step is the compiler's rather than a review's.
    /// 3. Add a match arm here (the compiler enforces exhaustiveness on
    ///    `FixCommand`, so missing this step is a build error).
    /// 4. Add a test asserting drift → fix → 0 findings.
    fn try_fix(&self, cmd: FixCommand) -> FixOutcome {
        match cmd {
            FixCommand::CodegenSync => {
                let Some(cfg) = self.config.codegen.as_ref() else {
                    return FixOutcome::Failed {
                        reason: "no [codegen] section in harness.toml".into(),
                    };
                };
                match SentinelSyncer::new(cfg, self.working_dir).sync() {
                    Ok(_) => FixOutcome::Applied,
                    Err(e) => FixOutcome::Failed {
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

        self.run_surface_validator::<RuleValidator<'_>>(
            &changed,
            &mut findings,
            &mut run,
            &mut skipped,
            &mut files_scanned,
        )?;
        self.run_surface_validator::<SkillValidator<'_>>(
            &changed,
            &mut findings,
            &mut run,
            &mut skipped,
            &mut files_scanned,
        )?;
        self.run_surface_validator::<AgentValidator<'_>>(
            &changed,
            &mut findings,
            &mut run,
            &mut skipped,
            &mut files_scanned,
        )?;
        self.run_surface_validator::<RoutineValidator>(
            &changed,
            &mut findings,
            &mut run,
            &mut skipped,
            &mut files_scanned,
        )?;
        self.run_surface_validator::<OutputStyleValidator<'_>>(
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
        self.run_governs(&mut findings, &mut run, &mut skipped)?;
        self.run_advisories(&mut findings, &mut run, &mut skipped)?;
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
        // `-z`: without it git quotes any path outside ASCII as octal escapes,
        // so a changed `.claude/rules/한글.md` never equals the candidate the
        // gate discovered and a windowed run reports clean over a file it
        // never opened.
        // `--relative`: a config below the git top level — `apps/web/harness.toml`
        // in a monorepo — is answered from the repository root otherwise, and
        // `apps/web/CLAUDE.md` never equals the candidate `CLAUDE.md` the gate
        // discovered from its own directory. Scanned nothing, reported clean.
        let listed = self.git_paths(&["diff", "--name-only", "-z", "--relative", since])?;
        Ok(Some(listed.into_iter().collect()))
    }

    /// The NUL-delimited paths a git listing prints, joined to the project.
    ///
    /// A path that is not UTF-8 is an error rather than a lossy decode: the
    /// replacement character names a file that does not exist, which the
    /// gate would then skip as absent while reporting the arm as run.
    fn git_paths(&self, args: &[&str]) -> Result<Vec<PathBuf>> {
        let command = format!("git {}", args.join(" "));
        let output = Command::new("git")
            .args(args)
            .current_dir(self.working_dir)
            .output()
            .map_err(|e| Error::CheckGitFailure {
                message: format!("{command} spawn: {e}"),
            })?;
        if !output.status.success() {
            return Err(Error::CheckGitFailure {
                message: format!(
                    "{command} failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        let raw = String::from_utf8(output.stdout).map_err(|_| Error::CheckGitFailure {
            message: format!("{command} listed a path that is not UTF-8"),
        })?;
        Ok(raw
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(|path| self.working_dir.join(path))
            .collect())
    }

    /// Every nested `CLAUDE.md` the project owns.
    ///
    /// Claude Code loads a nested `CLAUDE.md` when work happens under its
    /// directory, so its claims are as live as the root's — but a walk over
    /// the tree would also read the one a vendored package ships under
    /// `node_modules`, and resolve its paths against this project: a Blocker
    /// about a file nobody here wrote. The project's ignore files are the
    /// one non-heuristic answer to which files are its own, and only the
    /// project's: `--exclude-standard` would also read the developer's global
    /// excludes and `.git/info/exclude`, and a global `CLAUDE.md` line — a
    /// common habit for AI configuration — made the same commit pass on one
    /// machine and fail on another. A hook still has to see the untracked
    /// file its author just added, so the set is tracked plus untracked not
    /// ignored by `.gitignore`. A tracked file is the project's whatever its
    /// directory is called: committing a vendored `CLAUDE.md` makes its claims
    /// the project's, resolved like every other from the project root.
    fn nested_claude_md_files(&self) -> Result<Vec<PathBuf>> {
        self.git_paths(&[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-per-directory=.gitignore",
            "--",
            ":(glob)**/CLAUDE.md",
        ])
    }

    /// `claudeMdExcludes` from the project's settings, merged across the two
    /// project scopes the way the runtime merges them. A pattern the runtime
    /// could not honor is an error here too.
    fn claude_md_excludes(&self) -> Result<Vec<glob::Pattern>> {
        let mut patterns = Vec::new();
        for scope in [".claude/settings.json", ".claude/settings.local.json"] {
            let path = self.working_dir.join(scope);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                // Malformed settings are `validate.settings`' finding, not a
                // reason for this arm to guess at an exclude list.
                continue;
            };
            let Some(listed) = value.get("claudeMdExcludes").and_then(|v| v.as_array()) else {
                continue;
            };
            for raw in listed.iter().filter_map(|v| v.as_str()) {
                patterns.push(glob::Pattern::new(raw).map_err(|e| Error::ConfigInvalid {
                    message: format!("{scope}: claudeMdExcludes pattern '{raw}' is invalid: {e}"),
                    location: None,
                })?);
            }
        }
        Ok(patterns)
    }

    fn passes_filter(&self, path: &Path, changed: &Option<HashSet<PathBuf>>) -> bool {
        match changed {
            Some(set) => set.contains(path),
            None => true,
        }
    }

    fn discover_glob(&self, pattern: &str) -> Result<Vec<PathBuf>> {
        // The project's own path is a literal, not a pattern: `glob_root`
        // escapes it so a checkout under `repo [backup]` scans its files
        // rather than reporting a clean gate over nothing.
        let s = crate::glob_root::rooted(self.working_dir, pattern)?;
        // Surface glob failures rather than truncating to an empty list — a
        // dropped traversal error (permissions, symlink loop) would make a
        // validator falsely report clean on files it never scanned.
        let mut out = Vec::new();
        for entry in glob::glob(&s).map_err(|e| Error::IoFailure {
            path: self.working_dir.join(pattern),
            source: std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("glob: {e}")),
        })? {
            out.push(entry.map_err(|e| Error::IoFailure {
                path: e.path().to_path_buf(),
                source: e.into(),
            })?);
        }
        Ok(out)
    }

    /// Run one [`SurfaceValidator`] over the files its glob covers.
    ///
    /// Every glob-driven validator shares this body — the skipped-vs-ran
    /// contract, the `--since` filter, and the scanned-file count are stated
    /// once, so an artifact class added later cannot diverge from the others
    /// on any of them.
    fn run_surface_validator<V: SurfaceValidator<'a>>(
        &self,
        changed: &Option<HashSet<PathBuf>>,
        findings: &mut Vec<Finding>,
        run: &mut Vec<String>,
        skipped: &mut Vec<SkippedRule>,
        files_scanned: &mut usize,
    ) -> Result<()> {
        let Some(policy) = V::policy(self.config) else {
            skipped.push(SkippedRule {
                slug: V::SLUG.into(),
                reason: format!("no [{}] section", V::SLUG),
            });
            return Ok(());
        };
        let validator = V::build(policy);
        for path in &self.discover_glob(V::GLOB)? {
            if !self.passes_filter(path, changed) {
                continue;
            }
            *files_scanned += 1;
            findings.extend(validator.validate_path(path)?);
        }
        run.push(V::SLUG.into());
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
        // The project settings and its local override are independent files
        // with independent `--since` filter status. Check each on its own —
        // a change to `settings.local.json` alone must NOT be masked by
        // `settings.json` being absent from the diff.
        let project = self.working_dir.join(".claude/settings.json");
        let local = self.working_dir.join(".claude/settings.local.json");
        let mut considered = false;
        if project.is_file() {
            considered = true;
            if self.passes_filter(&project, changed) {
                findings.extend(
                    SettingsValidator::new().validate_file(&project, SettingsScope::Project)?,
                );
                *files_scanned += 1;
            }
        }
        if local.is_file() {
            considered = true;
            if self.passes_filter(&local, changed) {
                findings
                    .extend(SettingsValidator::new().validate_file(&local, SettingsScope::Local)?);
                *files_scanned += 1;
            }
        }
        if considered {
            run.push("validate.settings".into());
        } else {
            skipped.push(SkippedRule {
                slug: "validate.settings".into(),
                reason: ".claude/settings.json not present".into(),
            });
        }
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
        // Every surface this gate validates for shape: a file checked for
        // frontmatter and budget while its claims went unchecked reads as
        // verified. The globs are the validators' own, so the set a file is
        // shape-checked in is the set its claims are resolved in;
        // `check_reads_a_claim_from_every_shape_validated_surface` holds
        // this list to the validators `run` dispatches.
        //
        // The two project memory locations the runtime always reads are
        // unconditional. The nested set comes from git, and a git that
        // cannot answer — no repository, dubious ownership — leaves that set
        // declared unmeasured rather than the gate unrun.
        let mut candidates: Vec<PathBuf> = ["CLAUDE.md", ".claude/CLAUDE.md"]
            .iter()
            .map(|name| self.working_dir.join(name))
            .collect();
        match self.nested_claude_md_files() {
            Ok(nested) => candidates.extend(nested),
            Err(Error::CheckGitFailure { message }) => skipped.push(SkippedRule {
                slug: "evidence.nested-memory".into(),
                reason: message,
            }),
            Err(e) => return Err(e),
        }
        for glob in [
            <RuleValidator as SurfaceValidator>::GLOB,
            <SkillValidator as SurfaceValidator>::GLOB,
            <AgentValidator as SurfaceValidator>::GLOB,
            <OutputStyleValidator as SurfaceValidator>::GLOB,
            <RoutineValidator as SurfaceValidator>::GLOB,
        ] {
            candidates.extend(self.discover_glob(glob)?);
        }
        // `.claude/rules/CLAUDE.md` is a rule and a memory file; read once.
        candidates.sort();
        candidates.dedup();
        let excluded = self.claude_md_excludes()?;
        for path in &candidates {
            if !path.is_file() {
                continue;
            }
            if !self.passes_filter(path, changed) {
                continue;
            }
            // The runtime's own skip list: a memory file it never loads makes
            // no claim, whatever the tree holds.
            if path.file_name().is_some_and(|name| name == "CLAUDE.md")
                && let Ok(relative) = path.strip_prefix(self.working_dir)
                && excluded
                    .iter()
                    .any(|pattern| pattern.matches_path(relative))
            {
                continue;
            }
            *files_scanned += 1;
            findings.extend(verifier.verify_file(path, self.working_dir)?);
        }
        run.push("evidence".into());
        Ok(())
    }

    /// Every declared `governs.live_truth` still exists in the tree. Shape
    /// findings stay with the rule validator (one defect, one reporter);
    /// this arm asks only the question that needs the tree.
    ///
    /// Deliberately ignores `--since`, for the codegen arm's reason: the
    /// defect is created by a change to a declared TRUTH, not to the rule
    /// that declares it, so filtering rules by the diff window lets a
    /// deleted truth slip through as a false negative. The full sweep reads
    /// only the rules directory and is cheap; it also counts nothing into
    /// `files_scanned`, which tallies the windowed scan.
    fn run_governs(
        &self,
        findings: &mut Vec<Finding>,
        run: &mut Vec<String>,
        skipped: &mut Vec<SkippedRule>,
    ) -> Result<()> {
        if self
            .config
            .validate
            .as_ref()
            .and_then(|v| v.rules.as_ref())
            .is_none()
        {
            skipped.push(SkippedRule {
                slug: "governs".into(),
                reason: "no [validate.rules] section".into(),
            });
            return Ok(());
        }
        let auditor = crate::governs::GovernsAuditor::new(self.working_dir);
        for path in &self.discover_glob(<RuleValidator as SurfaceValidator>::GLOB)? {
            let content = std::fs::read_to_string(path).map_err(|e| Error::IoFailure {
                path: path.clone(),
                source: e,
            })?;
            findings.extend(auditor.audit_rule(&content, path));
        }
        run.push("governs".into());
        Ok(())
    }

    /// Every declared advisory's recorded evidence still describes its
    /// inputs. Ignores `--since` for the same reason the governs arm does:
    /// staleness is created by a change to a declared input, not to the
    /// baseline file, and the digests already answer the windowed question
    /// exactly. Counts nothing into `files_scanned`.
    fn run_advisories(
        &self,
        findings: &mut Vec<Finding>,
        run: &mut Vec<String>,
        skipped: &mut Vec<SkippedRule>,
    ) -> Result<()> {
        let Some(cfg) = self.config.evidence.as_ref() else {
            skipped.push(SkippedRule {
                slug: "advisory".into(),
                reason: "no [evidence] section".into(),
            });
            return Ok(());
        };
        let auditor =
            crate::evidence::advisory::AdvisoryAuditor::new(self.working_dir, cfg, self.unattended);
        findings.extend(auditor.audit()?);
        run.push("advisory".into());
        Ok(())
    }

    /// Codegen drift is checked **globally**, deliberately ignoring `--since`.
    /// A sentinel-block source edit can drift any number of targets, and the
    /// source→target mapping is not 1:1 with the changed-file set, so filtering
    /// by the diff window would let an upstream source change slip through as
    /// a false-negative (drift present, not reported). The full check is cheap
    /// (it reads only the configured groups), so it always runs in full.
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
                    fix_command: Some(FixCommand::CodegenSync),
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
        let perm_findings = PermissionAuditor::new(perms_policy, &allow, &ask, &deny)?.audit();
        for pf in &perm_findings {
            findings.push(Finding {
                slug: match pf.kind {
                    PermissionFindingKind::MissingBaselineDeny => {
                        "permission-missing-baseline-deny".into()
                    }
                    PermissionFindingKind::ContradictoryRule => {
                        "permission-contradictory-rule".into()
                    }
                },
                severity: Severity::Major,
                location: Location::file(settings_path.clone()),
                message: pf.message.clone(),
                hint: Some(
                    "regenerate from the canonical profiles: run `harnex policy permissions \
                     generate` and copy its `data` under `permissions` in .claude/settings.json. \
                     The profiles come from `[policy.permissions] profiles` in harness.toml — \
                     the subcommand takes no arguments."
                        .into(),
                ),
                auto_fixable: false,
                // No fix command: nothing in the safe-fix registry writes a
                // permission rule, and a rule to paste is what the hint above
                // already carries.
                fix_command: None,
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
