//! # Configuration: `harness.toml`
//!
//! Single source of truth for every project-specific shape. The toolkit
//! source contains zero hardcoded vocabularies — kind names, telemetry
//! payload schemas, provenance strategies, version pins, all derive from
//! `harness.toml`.
//!
//! [`Config::load`] walks upward from the working directory to find the
//! file, parses it, and runs [`Config::validate`]. A configuration that
//! the runtime cannot honor (duplicate names, unknown strategies,
//! unresolvable references, malformed schemas) is rejected at load.
//!
//! ## What this module refuses to do
//!
//! - Never silently coerce an unknown value to a default. Unknown
//!   strategy strings / enum values fail validation.
//! - Never accept a configuration whose values the toolkit itself
//!   would write but then reject (self-consistency invariant).
//! - Never embed project domain vocabulary in field defaults.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

static KIND_NAME_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9]([a-z0-9_-]*[a-z0-9])?$").expect("KIND_NAME_PATTERN"));

const CONFIG_FILE_NAME: &str = "harness.toml";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Config {
    pub meta: MetaConfig,
    #[serde(default)]
    pub kinds: Vec<KindDecl>,
    #[serde(default)]
    pub evidence: Option<EvidenceConfig>,
    #[serde(default)]
    pub telemetry: Option<TelemetryConfig>,
    #[serde(default)]
    pub codegen: Option<CodegenConfig>,
    #[serde(default)]
    pub policy: Option<PolicyConfig>,
    #[serde(default)]
    pub validate: Option<ValidateConfig>,
    #[serde(default)]
    pub lifecycle: Option<LifecycleConfig>,
    #[serde(default)]
    pub retirement: Option<RetirementConfig>,
    #[serde(default)]
    pub guard: Option<GuardConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MetaConfig {
    /// SemVer requirement that the binary must satisfy.
    pub harnex_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct KindDecl {
    pub name: String,
    pub glob: String,
    #[serde(default)]
    pub foundation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EvidenceConfig {
    #[serde(default = "default_provenance")]
    pub default_provenance: String,
    #[serde(default)]
    pub block_on_memory_only: bool,
    #[serde(default)]
    pub verifiers: Vec<VerifierDecl>,
}

fn default_provenance() -> String {
    "memory-only".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VerifierDecl {
    pub provenance: String,
    pub strategy: String,
    #[serde(default)]
    pub library_allowlist: Vec<String>,
    #[serde(default)]
    pub max_age_days: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TelemetryConfig {
    #[serde(default = "default_storage")]
    pub storage: String,
    pub storage_dir: PathBuf,
    #[serde(default = "default_rotate_at_mb")]
    pub rotate_at_mb: u32,
    #[serde(default)]
    pub kinds: Vec<TelemetryKindDecl>,
}

fn default_storage() -> String {
    "jsonl".to_string()
}
fn default_rotate_at_mb() -> u32 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TelemetryKindDecl {
    pub name: String,
    pub payload_schema: serde_json::Value,
}

// ---------- Codegen ----------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CodegenConfig {
    #[serde(default)]
    pub groups: Vec<CodegenGroupDecl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CodegenGroupDecl {
    pub name: String,
    pub source: PathBuf,
    pub source_key: String,
    /// Serialization format of the source file: `toml` | `json` | `yaml`.
    #[serde(default = "default_source_format")]
    pub source_format: String,
    #[serde(default)]
    pub targets: Vec<SentinelTargetDecl>,
}

fn default_source_format() -> String {
    "toml".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SentinelTargetDecl {
    pub path: PathBuf,
    pub begin: String,
    pub end: String,
    pub format: String,
    #[serde(default)]
    pub name: Option<String>,
}

// ---------- Policy ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicyConfig {
    #[serde(default)]
    pub permissions: Option<PermissionsPolicy>,
    #[serde(default)]
    pub versions: Vec<VersionPinDecl>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PermissionsPolicy {
    /// Names of built-in profiles to compose, applied in declaration order.
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub extra_allow: Vec<String>,
    #[serde(default)]
    pub extra_ask: Vec<String>,
    #[serde(default)]
    pub extra_deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VersionPinDecl {
    pub tool: String,
    pub version: String,
    /// `exact` | `minor` | `major` | `rolling`
    pub strategy: String,
    #[serde(default)]
    pub install_url: Option<String>,
}

// ---------- Validate ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ValidateConfig {
    #[serde(default)]
    pub rules: Option<RulesPolicy>,
    #[serde(default)]
    pub skills: Option<SkillsPolicy>,
    #[serde(default)]
    pub commit_msg: Option<CommitMsgPolicy>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CommitMsgPolicy {
    /// Trailer declarations. Each lists the trailer key (e.g.,
    /// `Nodex-Event`) and either a closed `allowed_values` set or
    /// `required = true` for free-text presence-only checking.
    #[serde(default)]
    pub trailers: Vec<CommitMsgTrailerDecl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CommitMsgTrailerDecl {
    /// Trailer key as it appears before the colon (case-sensitive).
    pub key: String,
    /// Closed set of permitted values. When omitted, any non-empty value
    /// is accepted (presence-only validation).
    #[serde(default)]
    pub allowed_values: Option<Vec<String>>,
    /// Whether the trailer must be present. Default: false (validate-if-present).
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RulesPolicy {
    #[serde(default = "default_rule_max_lines")]
    pub max_lines: usize,
    /// Rule slugs that may omit `paths:` frontmatter (always-loaded).
    #[serde(default)]
    pub always_loaded_slugs: Vec<String>,
}

fn default_rule_max_lines() -> usize {
    200
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SkillsPolicy {
    /// 5000-token compaction budget ≈ 500 lines.
    #[serde(default = "default_skill_max_lines")]
    pub max_skill_md_lines: usize,
    /// Claude Code skill listing budget caps `description + when_to_use` at
    /// 1536 chars; this is the project-level target (safe margin).
    #[serde(default = "default_skill_description_max")]
    pub max_description_chars: usize,
    /// Opt-in: emit a Major finding for any frontmatter key outside the
    /// Claude Code skill spec surface (`KNOWN_SKILL_KEYS`). Claude Code
    /// silently ignores unknown keys, so typos go undetected by default.
    #[serde(default)]
    pub reject_unknown_keys: bool,
    /// Opt-in: emit a Minor advisory when a skill description contains a
    /// side-effect verb (`commit`, `deploy`, `delete`, `submit`, `send`,
    /// `publish`, `release`) but lacks `disable-model-invocation: true`.
    /// Default off — the check matches prose, not intent, and produces
    /// false positives on read-only skills whose descriptions contain
    /// those verbs (e.g., a skill that *reviews* commits).
    #[serde(default)]
    pub flag_side_effect_verbs: bool,
}

fn default_skill_max_lines() -> usize {
    500
}
fn default_skill_description_max() -> usize {
    1536
}

// ---------- Lifecycle ----------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LifecycleConfig {
    #[serde(default = "default_promotion_min_instances")]
    pub promotion_min_instances: u32,
    #[serde(default = "default_promotion_min_days")]
    pub promotion_min_days: u32,
    #[serde(default = "default_stale_days")]
    pub stale_days: u32,
    #[serde(default = "default_silence_window_days")]
    pub silence_window_days: u32,
    #[serde(default = "default_grace_period_days")]
    pub grace_period_days: u32,
    #[serde(default = "default_observation_dir")]
    pub observation_dir: PathBuf,
    #[serde(default = "default_decision_dir")]
    pub decision_dir: PathBuf,
    #[serde(default)]
    pub consumer_detectors: Vec<ConsumerDetectorDecl>,
}

fn default_promotion_min_instances() -> u32 {
    3
}
fn default_promotion_min_days() -> u32 {
    30
}
fn default_stale_days() -> u32 {
    90
}
fn default_silence_window_days() -> u32 {
    90
}
fn default_grace_period_days() -> u32 {
    30
}
fn default_observation_dir() -> PathBuf {
    PathBuf::from(".harness/observations")
}
fn default_decision_dir() -> PathBuf {
    PathBuf::from(".harness/decisions")
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConsumerDetectorDecl {
    /// Kind name this detector applies to.
    pub kind: String,
    /// `grep`
    pub strategy: String,
    /// Template using `{slug}` placeholder.
    pub pattern: String,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RetirementConfig {
    #[serde(default)]
    pub exempt: RetirementExemptDecl,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RetirementExemptDecl {
    #[serde(default)]
    pub kinds: Vec<String>,
    #[serde(default)]
    pub slugs: Vec<String>,
}

// ---------- Guard ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GuardConfig {
    #[serde(default)]
    pub stop_audit: Option<StopAuditConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StopAuditConfig {
    /// Runtime name. Only `claude-code` is supported in v0.1.
    #[serde(default = "default_runtime")]
    pub runtime: String,
    /// Slash command of the critique skill to spawn (e.g. "/aix-critique").
    pub critique_skill: String,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Shell command + args that returns exit 0 when there are NO changes.
    /// Stop-audit spawns critique only when this command exits non-zero.
    #[serde(default)]
    pub has_changes_check: Vec<String>,
    /// Directory for the per-session retry counter ledger.
    #[serde(default = "default_audit_retry_dir")]
    pub retry_ledger_dir: PathBuf,
}

fn default_runtime() -> String {
    "claude-code".to_string()
}
fn default_max_retries() -> u32 {
    3
}
fn default_audit_retry_dir() -> PathBuf {
    PathBuf::from(".harness/_audit_retry")
}

impl Config {
    /// Load `harness.toml` by walking upward from `working_dir`.
    /// Returns the parsed + validated config and the resolved file path.
    pub fn load(working_dir: &Path) -> Result<(Self, PathBuf)> {
        let path = find_config_file(working_dir).ok_or_else(|| Error::ConfigNotFound {
            path: working_dir.join(CONFIG_FILE_NAME),
        })?;
        Self::load_from(&path).map(|cfg| (cfg, path))
    }

    /// Load + validate from a specific file. Lower-level than [`load`].
    pub fn load_from(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path).map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        let config: Config = toml::from_str(&contents).map_err(|e| Error::ConfigInvalid {
            message: format!("toml parse failure: {e}"),
            location: Some(crate::envelope::Location::file(path.to_path_buf())),
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Validate all cross-section invariants. Idempotent.
    pub fn validate(&self) -> Result<()> {
        self.validate_version()?;
        self.validate_kinds()?;
        self.validate_evidence()?;
        self.validate_telemetry()?;
        self.validate_codegen()?;
        self.validate_policy()?;
        self.validate_lifecycle()?;
        self.validate_guard()?;
        Ok(())
    }

    fn validate_version(&self) -> Result<()> {
        let raw = self.meta.harnex_version.trim();
        if raw.is_empty() {
            return Err(Error::ConfigInvalid {
                message: "[meta] harnex_version is empty".into(),
                location: None,
            });
        }
        let req = VersionReq::parse(raw).map_err(|e| Error::ConfigInvalid {
            message: format!("[meta] harnex_version '{raw}' is not a SemVer requirement: {e}"),
            location: None,
        })?;
        let actual_str = env!("CARGO_PKG_VERSION");
        let actual = Version::parse(actual_str).map_err(|e| Error::ConfigInvalid {
            message: format!("internal: own version {actual_str} unparseable: {e}"),
            location: None,
        })?;
        if !req.matches(&actual) {
            return Err(Error::ConfigVersionMismatch {
                required: raw.to_string(),
                actual: actual_str.to_string(),
            });
        }
        Ok(())
    }

    fn validate_kinds(&self) -> Result<()> {
        let mut seen = HashSet::new();
        for k in &self.kinds {
            if !KIND_NAME_PATTERN.is_match(&k.name) {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "[[kinds]] name '{}' must match [a-z0-9][a-z0-9_-]*[a-z0-9] (ASCII lowercase, digits, hyphens, underscores)",
                        k.name
                    ),
                    location: None,
                });
            }
            if !seen.insert(&k.name) {
                return Err(Error::ConfigInvalid {
                    message: format!("duplicate [[kinds]] name: {}", k.name),
                    location: None,
                });
            }
            glob::Pattern::new(&k.glob).map_err(|e| Error::ConfigInvalid {
                message: format!("[[kinds]] '{}' has invalid glob '{}': {e}", k.name, k.glob),
                location: None,
            })?;
        }
        Ok(())
    }

    fn validate_evidence(&self) -> Result<()> {
        let Some(ev) = &self.evidence else {
            return Ok(());
        };
        let mut seen = HashSet::new();
        for v in &ev.verifiers {
            if !seen.insert(&v.provenance) {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "duplicate [[evidence.verifiers]] provenance: {}",
                        v.provenance
                    ),
                    location: None,
                });
            }
            if crate::evidence::VerifierStrategy::from_str(&v.strategy).is_none() {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "[[evidence.verifiers]] '{}' has unknown strategy '{}' (known: {})",
                        v.provenance,
                        v.strategy,
                        crate::evidence::VerifierStrategy::ALL
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    location: None,
                });
            }
        }
        if !ev
            .verifiers
            .iter()
            .any(|v| v.provenance == ev.default_provenance)
        {
            return Err(Error::ConfigInvalid {
                message: format!(
                    "[evidence] default_provenance '{}' has no matching [[evidence.verifiers]] entry",
                    ev.default_provenance
                ),
                location: None,
            });
        }
        Ok(())
    }

    fn validate_telemetry(&self) -> Result<()> {
        let Some(t) = &self.telemetry else {
            return Ok(());
        };
        if crate::telemetry::StorageKind::from_str(&t.storage).is_none() {
            return Err(Error::ConfigInvalid {
                message: format!(
                    "[telemetry] storage '{}' is unknown (known: {})",
                    t.storage,
                    crate::telemetry::StorageKind::ALL
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                location: None,
            });
        }
        if t.rotate_at_mb == 0 {
            return Err(Error::ConfigInvalid {
                message: "[telemetry] rotate_at_mb must be > 0".into(),
                location: None,
            });
        }
        let mut seen = HashSet::new();
        for k in &t.kinds {
            if !seen.insert(&k.name) {
                return Err(Error::ConfigInvalid {
                    message: format!("duplicate [[telemetry.kinds]] name: {}", k.name),
                    location: None,
                });
            }
            // Fully validate the payload_schema at load via the same parser
            // the appender uses — type=object, well-formed `required` (array
            // of strings), and well-formed `properties` (known types). One
            // validation path, no partial inline duplicate.
            crate::telemetry::KindSchema::from_value(&k.payload_schema).map_err(|e| {
                Error::ConfigInvalid {
                    message: format!("[[telemetry.kinds]] '{}': {e}", k.name),
                    location: None,
                }
            })?;
        }
        Ok(())
    }

    fn validate_codegen(&self) -> Result<()> {
        let Some(cg) = &self.codegen else {
            return Ok(());
        };
        let mut group_names = HashSet::new();
        // Cycle detection compares LEXICALLY-NORMALIZED paths so equivalent
        // spellings (`./nodex.toml` vs `nodex.toml`) cannot evade the
        // source-is-target guard.
        let mut sources: HashSet<PathBuf> = HashSet::new();
        for group in &cg.groups {
            if !group_names.insert(&group.name) {
                return Err(Error::ConfigInvalid {
                    message: format!("duplicate [[codegen.groups]] name: {}", group.name),
                    location: None,
                });
            }
            if crate::codegen::SourceFormat::from_str(&group.source_format).is_none() {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "codegen group '{}' has unknown source_format '{}' (known: {})",
                        group.name,
                        group.source_format,
                        crate::codegen::SourceFormat::ALL
                            .iter()
                            .map(|f| f.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    location: None,
                });
            }
            if group.source_key.trim().is_empty() {
                return Err(Error::ConfigInvalid {
                    message: format!("codegen group '{}' has empty source_key", group.name),
                    location: None,
                });
            }
            sources.insert(normalize_lexical(&group.source));
            for target in &group.targets {
                if crate::codegen::RendererStrategy::from_str(&target.format).is_none() {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "codegen group '{}' target has unknown format '{}' (known: {})",
                            group.name,
                            target.format,
                            crate::codegen::RendererStrategy::ALL
                                .iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        location: None,
                    });
                }
                if target.begin.trim().is_empty() || target.end.trim().is_empty() {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "codegen group '{}' target '{}' has empty begin/end sentinel",
                            group.name,
                            target.path.display()
                        ),
                        location: None,
                    });
                }
            }
        }
        // Cycle: a target file must not be the source of any group.
        for group in &cg.groups {
            for target in &group.targets {
                if sources.contains(&normalize_lexical(&target.path)) {
                    return Err(Error::CodegenCycle {
                        path: target.path.clone(),
                    });
                }
            }
        }
        // Duplicate target sentinels across groups would create non-convergent sync.
        let mut target_sentinels: HashSet<(PathBuf, String, String)> = HashSet::new();
        for group in &cg.groups {
            for target in &group.targets {
                let key = (
                    target.path.clone(),
                    target.begin.clone(),
                    target.end.clone(),
                );
                if !target_sentinels.insert(key) {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "duplicate codegen target sentinel: {} ({} / {})",
                            target.path.display(),
                            target.begin,
                            target.end
                        ),
                        location: None,
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_policy(&self) -> Result<()> {
        let Some(p) = &self.policy else {
            return Ok(());
        };
        // Profile names must resolve — a typo (e.g. "basline") would
        // otherwise be silently skipped by the permission auditor, dropping
        // an intended guardrail with no failure signal. Fail at load instead.
        if let Some(perms) = &p.permissions {
            for name in &perms.profiles {
                if crate::policy::PermissionProfile::from_str(name).is_none() {
                    return Err(Error::PolicyProfileUnknown { name: name.clone() });
                }
            }
        }
        for v in &p.versions {
            match v.strategy.as_str() {
                "exact" | "minor" | "major" | "rolling" => {}
                other => {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "[[policy.versions]] tool '{}' has unknown strategy '{other}' (use exact|minor|major|rolling)",
                            v.tool
                        ),
                        location: None,
                    });
                }
            }
            if v.strategy != "rolling" {
                semver::Version::parse(&v.version).map_err(|e| Error::ConfigInvalid {
                    message: format!(
                        "[[policy.versions]] tool '{}' version '{}' is not SemVer: {e}",
                        v.tool, v.version
                    ),
                    location: None,
                })?;
            }
        }
        Ok(())
    }

    fn validate_lifecycle(&self) -> Result<()> {
        let Some(l) = &self.lifecycle else {
            return Ok(());
        };
        if l.promotion_min_instances == 0 {
            return Err(Error::ConfigInvalid {
                message: "[lifecycle] promotion_min_instances must be > 0".into(),
                location: None,
            });
        }
        let kind_names: HashSet<&String> = self.kinds.iter().map(|k| &k.name).collect();
        for d in &l.consumer_detectors {
            if !kind_names.contains(&d.kind) {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "[[lifecycle.consumer_detectors]] kind '{}' is not declared in [[kinds]]",
                        d.kind
                    ),
                    location: None,
                });
            }
            if crate::lifecycle::ConsumerStrategy::from_str(&d.strategy).is_none() {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "consumer detector for kind '{}' uses unknown strategy '{}' (known: {})",
                        d.kind,
                        d.strategy,
                        crate::lifecycle::ConsumerStrategy::ALL
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    location: None,
                });
            }
            if !d.pattern.contains("{slug}") {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "consumer detector for kind '{}' pattern must contain {{slug}}",
                        d.kind
                    ),
                    location: None,
                });
            }
        }
        Ok(())
    }

    fn validate_guard(&self) -> Result<()> {
        let Some(g) = &self.guard else {
            return Ok(());
        };
        if let Some(sa) = &g.stop_audit {
            if sa.runtime != "claude-code" {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "[guard.stop_audit] runtime '{}' unsupported (only 'claude-code' in v0.1)",
                        sa.runtime
                    ),
                    location: None,
                });
            }
            if sa.critique_skill.trim().is_empty() {
                return Err(Error::ConfigInvalid {
                    message: "[guard.stop_audit] critique_skill is empty".into(),
                    location: None,
                });
            }
            // Bound the retry ceiling: 0 would escalate before the critique
            // ever runs, and a value at the integer ceiling would make the
            // `attempt > max_retries` escalation (and the corrupt-ledger
            // fail-safe) unreachable.
            if sa.max_retries == 0 || sa.max_retries > 100 {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "[guard.stop_audit] max_retries must be in 1..=100 (got {})",
                        sa.max_retries
                    ),
                    location: None,
                });
            }
        }
        Ok(())
    }
}

/// Lexically normalize a relative path for equality comparison: drop `.`
/// components and redundant separators without touching the filesystem.
/// `./nodex.toml`, `nodex.toml`, and `dir/../nodex.toml` are NOT all
/// collapsed (no `..` resolution — that needs the real tree); this only
/// removes `CurDir` segments, which is the spelling difference that evades
/// the codegen cycle guard.
fn normalize_lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

fn find_config_file(working_dir: &Path) -> Option<PathBuf> {
    let mut current = working_dir.to_path_buf();
    loop {
        let candidate = current.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn parse(src: &str) -> Result<Config> {
        let cfg: Config = toml::from_str(src).map_err(|e| Error::ConfigInvalid {
            message: e.to_string(),
            location: None,
        })?;
        cfg.validate()?;
        Ok(cfg)
    }

    #[test]
    fn loads_minimal_valid_config() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
        "#;
        let cfg = parse(src).unwrap();
        assert!(cfg.kinds.is_empty());
        assert!(cfg.evidence.is_none());
        assert!(cfg.telemetry.is_none());
    }

    #[test]
    fn rejects_unparseable_version() {
        let src = r#"
            [meta]
            harnex_version = "this-is-not-semver"
        "#;
        assert_eq!(parse(src).unwrap_err().code(), ErrorCode::ConfigInvalid);
    }

    #[test]
    fn rejects_out_of_range_stop_audit_max_retries() {
        for bad in ["0", "101"] {
            let src = format!(
                r#"
                [meta]
                harnex_version = ">=0.1, <0.2"
                [guard.stop_audit]
                critique_skill = "/critique"
                max_retries = {bad}
                "#
            );
            assert_eq!(
                parse(&src).unwrap_err().code(),
                ErrorCode::ConfigInvalid,
                "max_retries={bad} must be rejected"
            );
        }
    }

    #[test]
    fn accepts_in_range_stop_audit_max_retries() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [guard.stop_audit]
            critique_skill = "/critique"
            max_retries = 3
        "#;
        assert!(parse(src).is_ok());
    }

    #[test]
    fn rejects_version_outside_range() {
        let src = r#"
            [meta]
            harnex_version = ">=9.0, <10.0"
        "#;
        assert_eq!(
            parse(src).unwrap_err().code(),
            ErrorCode::ConfigVersionMismatch
        );
    }

    #[test]
    fn rejects_duplicate_kind() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [[kinds]]
            name = "rule"
            glob = "*.md"
            [[kinds]]
            name = "rule"
            glob = "*.txt"
        "#;
        let err = parse(src).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid);
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn rejects_unknown_verifier_strategy() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [evidence]
            default_provenance = "memory-only"
            [[evidence.verifiers]]
            provenance = "memory-only"
            strategy = "made-up-strategy"
        "#;
        let err = parse(src).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid);
        assert!(err.to_string().contains("unknown strategy"));
    }

    #[test]
    fn rejects_default_provenance_unregistered() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [evidence]
            default_provenance = "nope"
            [[evidence.verifiers]]
            provenance = "internal"
            strategy = "file-path-line"
        "#;
        let err = parse(src).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid);
        assert!(err.to_string().contains("default_provenance"));
    }

    #[test]
    fn rejects_telemetry_kind_with_non_object_schema() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [telemetry]
            storage = "jsonl"
            storage_dir = ".harness/telemetry"
            [[telemetry.kinds]]
            name = "broken"
            payload_schema = "not-an-object"
        "#;
        let err = parse(src).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid);
    }

    #[test]
    fn accepts_full_valid_config() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"

            [[kinds]]
            name = "rule"
            glob = ".claude/rules/*.md"

            [evidence]
            default_provenance = "memory-only"
            block_on_memory_only = true

            [[evidence.verifiers]]
            provenance = "internal"
            strategy = "file-path-line"

            [[evidence.verifiers]]
            provenance = "memory-only"
            strategy = "memory-only"

            [telemetry]
            storage = "jsonl"
            storage_dir = ".harness/telemetry"

            [[telemetry.kinds]]
            name = "skill-invoked"

            [telemetry.kinds.payload_schema]
            type = "object"
            required = ["skill", "outcome"]

            [telemetry.kinds.payload_schema.properties.skill]
            type = "string"

            [telemetry.kinds.payload_schema.properties.outcome]
            type = "string"
            enum = ["ok", "warn", "fail"]
        "#;
        let cfg = parse(src).unwrap();
        assert_eq!(cfg.kinds.len(), 1);
        assert_eq!(cfg.evidence.unwrap().verifiers.len(), 2);
        assert_eq!(cfg.telemetry.unwrap().kinds.len(), 1);
    }

    #[test]
    fn rejects_duplicate_codegen_target_sentinel() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"

            [codegen]
            [[codegen.groups]]
            name = "group-a"
            source = "source.toml"
            source_key = "values"
            [[codegen.groups.targets]]
            path = "target.md"
            begin = "<!-- BEGIN:x -->"
            end = "<!-- END:x -->"
            format = "markdown-bullet-list"

            [[codegen.groups]]
            name = "group-b"
            source = "other.toml"
            source_key = "values"
            [[codegen.groups.targets]]
            path = "target.md"
            begin = "<!-- BEGIN:x -->"
            end = "<!-- END:x -->"
            format = "markdown-bullet-list"
        "#;
        let err = parse(src).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid);
        assert!(
            err.to_string()
                .contains("duplicate codegen target sentinel")
        );
    }

    #[test]
    fn detects_codegen_cycle_across_path_spellings() {
        // `./nodex.toml` (target) vs `nodex.toml` (source) are the same file;
        // lexical normalization must catch the cycle despite the spelling.
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"

            [codegen]
            [[codegen.groups]]
            name = "group-a"
            source = "nodex.toml"
            source_key = "values"
            [[codegen.groups.targets]]
            path = "./nodex.toml"
            begin = "<!-- BEGIN:x -->"
            end = "<!-- END:x -->"
            format = "markdown-bullet-list"
        "#;
        assert_eq!(parse(src).unwrap_err().code(), ErrorCode::CodegenCycle);
    }

    #[test]
    fn rejects_telemetry_required_non_string() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [telemetry]
            storage = "jsonl"
            storage_dir = ".harness/telemetry"
            [[telemetry.kinds]]
            name = "k"
            [telemetry.kinds.payload_schema]
            type = "object"
            required = ["ok", 123]
            [telemetry.kinds.payload_schema.properties.ok]
            type = "string"
        "#;
        assert_eq!(parse(src).unwrap_err().code(), ErrorCode::ConfigInvalid);
    }

    #[test]
    fn rejects_telemetry_unknown_property_type() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [telemetry]
            storage = "jsonl"
            storage_dir = ".harness/telemetry"
            [[telemetry.kinds]]
            name = "k"
            [telemetry.kinds.payload_schema]
            type = "object"
            [telemetry.kinds.payload_schema.properties.f]
            type = "garbage"
        "#;
        assert_eq!(parse(src).unwrap_err().code(), ErrorCode::ConfigInvalid);
    }

    #[test]
    fn rejects_unknown_codegen_source_format() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"

            [codegen]
            [[codegen.groups]]
            name = "group-a"
            source = "source.xml"
            source_key = "values"
            source_format = "xml"
            [[codegen.groups.targets]]
            path = "target.md"
            begin = "<!-- BEGIN:x -->"
            end = "<!-- END:x -->"
            format = "markdown-bullet-list"
        "#;
        let err = parse(src).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid);
        assert!(err.to_string().contains("unknown source_format"));
    }

    #[test]
    fn rejects_empty_codegen_source_key() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"

            [codegen]
            [[codegen.groups]]
            name = "group-a"
            source = "source.toml"
            source_key = ""
            [[codegen.groups.targets]]
            path = "target.md"
            begin = "<!-- BEGIN:x -->"
            end = "<!-- END:x -->"
            format = "markdown-bullet-list"
        "#;
        let err = parse(src).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid);
        assert!(err.to_string().contains("empty source_key"));
    }

    #[test]
    fn rejects_unknown_permission_profile() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [policy.permissions]
            profiles = ["baseline", "basline"]
        "#;
        let err = parse(src).unwrap_err();
        assert_eq!(err.code(), ErrorCode::PolicyProfileUnknown);
    }

    #[test]
    fn accepts_known_permission_profiles() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [policy.permissions]
            profiles = ["baseline", "python-dev"]
        "#;
        assert!(parse(src).is_ok());
    }

    #[test]
    fn rejects_unicode_kind_name() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [[kinds]]
            name = "日本語"
            glob = "*.md"
        "#;
        let err = parse(src).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid);
    }

    #[test]
    fn accepts_valid_kind_names() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [[kinds]]
            name = "my-kind-2"
            glob = "*.md"
        "#;
        parse(src).unwrap();
    }
}
