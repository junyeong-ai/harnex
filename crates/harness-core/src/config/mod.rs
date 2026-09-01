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
    #[serde(default)]
    pub session: Option<SessionConfig>,
}

/// Where Claude Code transcripts live, and what reading them is parameterised by.
///
/// `roots` has no default on purpose. It is a machine-global path, and a
/// built-in one would put the author's layout into a binary that runs on other
/// machines — Constitution VII, applied to a path rather than a threshold.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    pub roots: Vec<String>,
    /// Shortest paragraph that counts as a repeatable block. Below it, ordinary
    /// sentence fragments recur between unrelated prompts and the repeat signal
    /// drowns.
    ///
    /// The default is a length, and a length carries different amounts of
    /// meaning per language: a script that writes a word in one or two
    /// characters says the same instruction in a third of the characters an
    /// alphabetic one needs, so the same floor hides more of what was written.
    /// A project working in one lowers it here rather than reading the absence
    /// as nothing repeated.
    #[serde(default = "default_min_block_chars")]
    pub min_block_chars: usize,
    /// Share of person-attributed turns that must carry a recognised prompt
    /// source before rate-reporting commands will answer.
    #[serde(default = "default_coverage_floor")]
    pub coverage_floor: f64,
    /// Observations a rate needs on both sides before a comparison subtracts
    /// them. Under it the two rates are still reported and only the difference
    /// is withheld, because a handful of submissions moves a rate by arrival
    /// order more than by anything the operator changed.
    #[serde(default = "default_min_support")]
    pub min_support: u64,
    /// Append-only ledger of measured windows, relative to `harness.toml`.
    #[serde(default = "default_baseline_path")]
    pub baseline_path: PathBuf,
    /// Cap on instructions returned one at a time, for callers that pay per
    /// instruction. Absent returns the window whole.
    #[serde(default)]
    pub submission_sample: Option<usize>,
    /// What a baseline treats as the harness, relative to the project it was
    /// scoped to. A comparison reports whether these moved between two
    /// windows, which is the difference between a delta that could be an
    /// effect and one that cannot.
    ///
    /// These are git pathspecs, so a bare name is anchored at the work tree
    /// root and reaches no deeper: project memory nested beside the code it
    /// governs takes the `**/` form alongside it, which is why the default
    /// carries `CLAUDE.md` twice.
    ///
    /// The default covers everywhere a scaffolded harness lands, held there by
    /// `every_scaffolded_artifact_is_inside_the_default_harness`. A project
    /// that keeps any of it elsewhere declares that here.
    #[serde(default = "default_harness_paths")]
    pub harness_paths: Vec<String>,
}

/// Everywhere a scaffolded harness lands, which is what a window measures the
/// change to when nothing else is declared.
pub fn default_harness_paths() -> Vec<String> {
    [
        ".claude",
        "CLAUDE.md",
        "**/CLAUDE.md",
        "harness.toml",
        "hooks",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}
fn default_min_block_chars() -> usize {
    40
}
fn default_coverage_floor() -> f64 {
    0.95
}
fn default_min_support() -> u64 {
    30
}
fn default_baseline_path() -> PathBuf {
    PathBuf::from(".harness/session-baselines.jsonl")
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MetaConfig {
    /// SemVer requirement that the binary must satisfy.
    pub harnex_version: String,
}

/// A class of artifact the retirement sweep walks. Closed (Article V): a
/// misspelled key here would load clean and drop what it was meant to set —
/// and `invocation_kind` misspelled drops a kind's whole silence measurement
/// with no error, which is the failure the measurement exists to prevent.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KindDecl {
    pub name: String,
    pub glob: String,
    #[serde(default)]
    pub foundation: bool,
    /// The telemetry Kind recording invocations of THIS kind's artifacts —
    /// the oracle the retirement sweep reads their silence from, matching a
    /// slug against the payload. Declare it only where the record can name
    /// them: an artifact that is loaded rather than invoked (a rule) is never
    /// in an invocation record, so reading its absence there would convict
    /// every one of them the moment anything else runs. Undeclared, this
    /// kind's slugs are `unmeasured` and the Silent signal never fires for
    /// them. The glob must yield the name the record uses as each file's
    /// stem.
    #[serde(default)]
    pub invocation_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EvidenceConfig {
    #[serde(default = "default_provenance")]
    pub default_provenance: String,
    #[serde(default)]
    pub block_on_memory_only: bool,
    #[serde(default)]
    pub verifiers: Vec<VerifierDecl>,
    /// Directory the recorded advisory baselines live in, project-relative.
    /// Changing it strands baselines at the old path — move them with it, or
    /// they sit unscanned where not even the orphan finding looks.
    #[serde(default = "default_advisory_dir")]
    pub advisory_dir: String,
    #[serde(default)]
    pub advisories: Vec<AdvisoryDecl>,
}

fn default_advisory_dir() -> String {
    "evidence".to_string()
}

/// One declared advisory: a measurement this toolkit never runs, whose
/// recorded evidence it holds fresh. The advisory's own findings never gate;
/// the freshness of its basis does.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdvisoryDecl {
    /// Kebab-case identity; the baseline lands at `<advisory_dir>/<id>.json`.
    pub id: String,
    /// Paths whose change invalidates the evidence — what the measurement
    /// was ABOUT. Literal project-relative paths; a directory covers its
    /// subtree.
    pub inputs: Vec<String>,
    /// Paths of the measuring instrument itself, digested apart from the
    /// inputs so a stale finding can say which of the two moved. Only
    /// in-tree content is identity — a wrapper that shells out re-points
    /// with no tree diff, so declare its lockfile or version pin beside it.
    #[serde(default)]
    pub engine: Vec<String>,
    /// Whether an unattended context (a push gate, `--unattended`) may block
    /// on this entry's staleness. Only where the person being blocked can
    /// re-measure in the same sitting; elsewhere staleness reports without
    /// gating there.
    #[serde(default)]
    pub unattended_remeasure: bool,
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
    pub routines: Option<RoutinesPolicy>,
    #[serde(default)]
    pub rules: Option<RulesPolicy>,
    #[serde(default)]
    pub skills: Option<SkillsPolicy>,
    #[serde(default)]
    pub agents: Option<AgentsPolicy>,
    #[serde(default)]
    pub output_styles: Option<OutputStylesPolicy>,
    #[serde(default)]
    pub commit_msg: Option<CommitMsgPolicy>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OutputStylesPolicy {
    /// Opt-in: emit a Major finding for any frontmatter key outside the
    /// Claude Code output-style spec surface (`KNOWN_OUTPUT_STYLE_KEYS`).
    /// Default off — a hardcoded key list lags the upstream spec, and a stale
    /// list turns valid frontmatter into a finding.
    #[serde(default)]
    pub reject_unknown_keys: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentsPolicy {
    /// Opt-in: emit a Major finding for any frontmatter key outside the
    /// Claude Code sub-agent spec surface (`KNOWN_AGENT_KEYS`). Claude Code
    /// silently ignores an unknown key, so a typo costs the field it was
    /// meant to set. Default off — a hardcoded key list lags the upstream
    /// spec, and a stale list turns valid frontmatter into a finding.
    #[serde(default)]
    pub reject_unknown_keys: bool,
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

/// Enables `.claude/routines/*.md` shape validation. No knobs yet: the
/// grammar is closed and the schedule states are the query's, not a
/// policy's.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RoutinesPolicy {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RulesPolicy {
    /// Line budget for the always-loaded set — the rules that carry no
    /// `paths:` and therefore enter every session's context. 200 is the
    /// Claude Code memory spec's target for a file loaded unconditionally.
    #[serde(default = "default_rule_max_lines")]
    pub max_lines: usize,
    /// Opt-in line budget for path-scoped rules, which load only when a
    /// matching file is read. `None` (the default) leaves them unbounded:
    /// a cohesive long rule that costs context only on its own paths is not
    /// a defect, and a fixed ceiling over that set blocks more than it
    /// protects. Set it when the project wants a review-for-domain-mixing
    /// prompt; the finding is advisory either way.
    #[serde(default)]
    pub max_scoped_lines: Option<usize>,
    /// Rule slugs that may omit `paths:` frontmatter (always-loaded).
    #[serde(default)]
    pub always_loaded_slugs: Vec<String>,
    /// Opt-in: every path-scoped rule must carry a `governs:` declaration
    /// (`harness_core::governs`). Always-loaded rules are exempt — the
    /// declaration's consumers act where a rule crosses a load boundary,
    /// which a rule in every context never does, and forcing a declaration
    /// onto a rule whose truth is not in the tree invites a dishonest one.
    /// A declaration present on any rule is shape-validated regardless.
    #[serde(default)]
    pub require_governs: bool,
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
    #[serde(default)]
    pub floor: Option<FloorConfig>,
}

/// The enforcement-surface freeze (`guard::floor`). Declaring the section is
/// what turns the floor on; `protected_paths` adds the project's own
/// gate-defining files to the built-in set the module documents.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FloorConfig {
    /// Repo-relative literal paths; a trailing `/` freezes a directory.
    #[serde(default)]
    pub protected_paths: Vec<String>,
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

impl StopAuditConfig {
    /// The command that answers whether this session left work behind, and so
    /// whether stopping costs a model call.
    ///
    /// Required, because the alternative is answering it without looking:
    /// declaring the section already commits to spawning a critique, and an
    /// unstated gate would spawn one at every Stop of every session. What that
    /// probe should be is a project fact — which trees, which staging state,
    /// which submodules — so it is asked for rather than guessed.
    pub fn changes_probe(&self) -> Result<(&str, &[String])> {
        let invalid = |message: &str| Error::ConfigInvalid {
            message: format!("[guard.stop_audit] has_changes_check {message}"),
            location: None,
        };
        let (program, args) = self.has_changes_check.split_first().ok_or_else(|| {
            invalid(
                "is empty — the section spawns a critique per Stop, so the command \
                 that decides when must be stated (e.g. [\"git\", \"diff\", \"--quiet\"])",
            )
        })?;
        // Whether a named program is installed is the runner's answer, not
        // this machine's; a blank name is no program anywhere.
        if program.trim().is_empty() {
            return Err(invalid("names no program"));
        }
        Ok((program.as_str(), args))
    }
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
        self.validate_session()?;
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
            if let Some(invocation) = &k.invocation_kind {
                if !self
                    .telemetry
                    .as_ref()
                    .is_some_and(|t| t.kinds.iter().any(|d| &d.name == invocation))
                {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "[[kinds]] '{}' invocation_kind '{invocation}' is not declared in [[telemetry.kinds]]",
                            k.name
                        ),
                        location: None,
                    });
                }
                // A slug is a match's file stem, so a glob whose final
                // component is a literal while an earlier one is not gives
                // every match the same slug by construction — the record names
                // artifacts individually and this kind cannot tell them apart.
                // Pure glob algebra: a wildcard final component varies the
                // stem, and a wholly literal glob matches at most one file.
                if let Some((last, earlier)) = k.glob.split('/').collect::<Vec<_>>().split_last()
                    && !has_glob_meta(last)
                    && earlier.iter().any(|c| has_glob_meta(c))
                {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "[[kinds]] '{}' declares invocation_kind but its glob '{}' gives every match the slug '{}' — match the artifact whose name the record uses (e.g. the directory), not a fixed filename inside it",
                            k.name,
                            k.glob,
                            std::path::Path::new(last)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or(last)
                        ),
                        location: None,
                    });
                }
            }
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
        if !crate::path_guard::literal_relative(&ev.advisory_dir) {
            return Err(Error::ConfigInvalid {
                message: format!(
                    "[evidence] advisory_dir '{}' is not a literal project-relative path",
                    ev.advisory_dir
                ),
                location: None,
            });
        }
        let mut advisory_ids = HashSet::new();
        for advisory in &ev.advisories {
            if advisory.id.is_empty()
                || !advisory
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "[[evidence.advisories]] id '{}' is not kebab-case — it names the \
                         baseline file",
                        advisory.id
                    ),
                    location: None,
                });
            }
            if !advisory_ids.insert(&advisory.id) {
                return Err(Error::ConfigInvalid {
                    message: format!("duplicate [[evidence.advisories]] id: {}", advisory.id),
                    location: None,
                });
            }
            if advisory.inputs.is_empty() {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "[[evidence.advisories]] '{}' declares no inputs — evidence with no \
                         inputs can never go stale, which is a claim, not a measurement",
                        advisory.id
                    ),
                    location: None,
                });
            }
            for path in advisory.inputs.iter().chain(&advisory.engine) {
                if !crate::path_guard::literal_relative(path) {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "[[evidence.advisories]] '{}' path '{path}' is not a literal \
                             project-relative path",
                            advisory.id
                        ),
                        location: None,
                    });
                }
                let own_baseline = format!("{}/{}.json", ev.advisory_dir, advisory.id);
                let covers_dir = ev.advisory_dir == *path
                    || ev
                        .advisory_dir
                        .strip_prefix(path.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
                    || *path == own_baseline;
                if covers_dir {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "[[evidence.advisories]] '{}' declares '{path}', which contains \
                             its own basis — recording would change what it measured and the \
                             evidence could never be fresh (advisory_dir '{}')",
                            advisory.id, ev.advisory_dir
                        ),
                        location: None,
                    });
                }
            }
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
            // Telemetry kind names are used directly as ledger filenames
            // (`{name}.jsonl`), so they share the same safe-name contract as
            // [[kinds]] — a `/` or `..` would otherwise escape the storage dir.
            if !KIND_NAME_PATTERN.is_match(&k.name) {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "[[telemetry.kinds]] name '{}' must match [a-z0-9][a-z0-9_-]*[a-z0-9] (ASCII lowercase, digits, hyphens, underscores)",
                        k.name
                    ),
                    location: None,
                });
            }
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
            // Source and target paths must stay inside the project: reject
            // both `..` traversal AND absolute paths (an absolute path wins a
            // `join`, silently escaping working_dir). The runtime write/read
            // guard would reject these at execution time, so per Article IV
            // reject them at load rather than deferring to a runtime failure
            // on an otherwise "valid" config.
            if crate::path_guard::reject_traversal(&group.source).is_err()
                || group.source.is_absolute()
            {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "codegen group '{}' source '{}' must be project-relative (no `..`, no leading `/`)",
                        group.name,
                        group.source.display()
                    ),
                    location: None,
                });
            }
            sources.insert(normalize_lexical(&group.source));
            for target in &group.targets {
                if crate::path_guard::reject_traversal(&target.path).is_err()
                    || target.path.is_absolute()
                {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "codegen group '{}' target '{}' must be project-relative (no `..`, no leading `/`)",
                            group.name,
                            target.path.display()
                        ),
                        location: None,
                    });
                }
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
        // Duplicate target sentinels across groups would create non-convergent
        // sync. Key on the LEXICALLY-NORMALIZED path (as the cycle check above
        // does) so `target.md` and `./target.md` are recognized as the same
        // file rather than slipping past as distinct keys.
        let mut target_sentinels: HashSet<(PathBuf, String, String)> = HashSet::new();
        for group in &cg.groups {
            for target in &group.targets {
                let key = (
                    normalize_lexical(&target.path),
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
            // A rule Claude Code accepts and never consults is a guardrail the
            // runtime cannot honor: it merges into the generated settings,
            // reads as a floor, and enforces nothing.
            for (field, rules, direction) in [
                (
                    "[policy.permissions].extra_allow",
                    &perms.extra_allow,
                    crate::policy::RuleDirection::Allow,
                ),
                (
                    "[policy.permissions].extra_ask",
                    &perms.extra_ask,
                    crate::policy::RuleDirection::Ask,
                ),
                (
                    "[policy.permissions].extra_deny",
                    &perms.extra_deny,
                    crate::policy::RuleDirection::Deny,
                ),
            ] {
                for rule in rules {
                    let parsed = crate::policy::PermissionRule::parse(rule);
                    if let crate::policy::RuleEffect::Inert(inert) = parsed.effect() {
                        return Err(Error::PolicyRuleInert {
                            field,
                            rule: rule.clone(),
                            reason: inert.reason_text(),
                            hint: inert.hint(),
                        });
                    }
                    // Refused where a settings file only advises: an extra is
                    // intent declared for generation, and generating a rule
                    // the operator will misread is a config the runtime
                    // cannot honor as written.
                    if let Some(misleading) = parsed.misleading(direction) {
                        return Err(Error::PolicyRuleMisleading {
                            field,
                            rule: rule.clone(),
                            reason: misleading.reason_text(),
                            hint: misleading.hint(),
                        });
                    }
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
            // Each exclude_glob must compile, or the runtime silently drops
            // it (`.ok()`) and the exclusion never takes effect. `{slug}` is a
            // runtime placeholder; substitute a sentinel before compiling so
            // the glob alternation parser doesn't mask a structural error.
            for g in &d.exclude_globs {
                let probe = g.replace("{slug}", "slug");
                glob::Pattern::new(&probe).map_err(|e| Error::ConfigInvalid {
                    message: format!(
                        "consumer detector for kind '{}' has invalid exclude_glob '{g}': {e}",
                        d.kind
                    ),
                    location: None,
                })?;
            }
        }
        Ok(())
    }

    fn validate_session(&self) -> Result<()> {
        let Some(sess) = &self.session else {
            return Ok(());
        };
        if sess.roots.is_empty() {
            return Err(Error::ConfigInvalid {
                message: "[session] roots is empty; declare where transcripts live (there is no built-in default)".into(),
                location: None,
            });
        }
        if let Some(blank) = sess.roots.iter().find(|r| r.trim().is_empty()) {
            return Err(Error::ConfigInvalid {
                message: format!("[session] roots contains a blank entry ({blank:?})"),
                location: None,
            });
        }
        if sess.min_block_chars == 0 {
            return Err(Error::ConfigInvalid {
                message: "[session] min_block_chars is 0; every paragraph would count as repeated"
                    .into(),
                location: None,
            });
        }
        if !(0.0..=1.0).contains(&sess.coverage_floor) {
            return Err(Error::ConfigInvalid {
                message: format!(
                    "[session] coverage_floor {} is outside 0.0..=1.0",
                    sess.coverage_floor
                ),
                location: None,
            });
        }
        if let Some(bad) = sess
            .harness_paths
            .iter()
            .find(|p| p.trim().is_empty() || std::path::Path::new(p).is_absolute())
        {
            return Err(Error::ConfigInvalid {
                message: format!(
                    "[session] harness_paths entry '{bad}' is empty or absolute; each is relative to the project a window is scoped to"
                ),
                location: None,
            });
        }
        if sess.min_support == 0 {
            return Err(Error::ConfigInvalid {
                message: "[session] min_support is 0; a rate over nothing would be compared".into(),
                location: None,
            });
        }
        if sess.submission_sample == Some(0) {
            return Err(Error::ConfigInvalid {
                message: "[session] submission_sample is 0; omit it to return the window whole"
                    .into(),
                location: None,
            });
        }
        if sess.baseline_path.as_os_str().is_empty() {
            return Err(Error::ConfigInvalid {
                message:
                    "[session] baseline_path is empty; name the ledger measured windows append to"
                        .into(),
                location: None,
            });
        }
        // `path_guard` refuses a traversal at write time; a configuration the
        // runtime cannot honor is rejected at load instead, so the refusal
        // arrives before a window is measured rather than after.
        if sess
            .baseline_path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        {
            return Err(Error::ConfigInvalid {
                message: format!(
                    "[session] baseline_path {} climbs out of the project; a write refuses a '..' segment",
                    sess.baseline_path.display()
                ),
                location: None,
            });
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
            sa.changes_probe()?;
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
        if let Some(floor) = &g.floor {
            let mut seen: Vec<String> = Vec::new();
            for entry in &floor.protected_paths {
                let literal = entry.strip_suffix('/').unwrap_or(entry);
                if literal.is_empty() || !crate::path_guard::literal_relative(literal) {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "[guard.floor] protected_paths entry '{entry}' is not a literal \
                             repo-relative path (a trailing / marks a directory)"
                        ),
                        location: None,
                    });
                }
                // Matching is case-insensitive, so uniqueness is too; and the
                // built-in comparison is against the file, not its optional
                // trailing slash, so `harness.toml/` cannot restate it either.
                let lower = literal.to_lowercase();
                if crate::guard::floor::BUILT_IN_PROTECTED.contains(&lower.as_str()) {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "[guard.floor] protected_paths entry '{entry}' is built into the \
                             floor — declaring it again is a second copy that can drift"
                        ),
                        location: None,
                    });
                }
                if seen.contains(&lower) {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "[guard.floor] protected_paths entry '{entry}' is duplicated"
                        ),
                        location: None,
                    });
                }
                seen.push(lower);
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
/// Whether one glob path component can match more than one literal name.
fn has_glob_meta(component: &str) -> bool {
    component.contains(['*', '?', '['])
}

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
    fn rejects_a_baseline_path_that_climbs_out_of_the_project() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [session]
            roots = ["~/.claude/projects"]
            baseline_path = "../elsewhere/ledger.jsonl"
        "#;
        assert_eq!(parse(src).unwrap_err().code(), ErrorCode::ConfigInvalid);
    }

    #[test]
    fn rejects_a_session_field_it_does_not_declare() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [session]
            roots = ["~/.claude/projects"]
            submission_sampl = 50
        "#;
        assert!(
            parse(src).is_err(),
            "a typo must not fall back to the default it shadows"
        );
    }

    #[test]
    fn session_roots_have_no_default_because_the_path_is_machine_global() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [session]
        "#;
        assert_eq!(parse(src).unwrap_err().code(), ErrorCode::ConfigInvalid);
    }

    #[test]
    fn rejects_a_blank_session_root() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [session]
            roots = ["~/.claude/projects", "  "]
        "#;
        assert_eq!(parse(src).unwrap_err().code(), ErrorCode::ConfigInvalid);
    }

    #[test]
    fn rejects_a_zero_min_block_chars() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [session]
            roots = ["~/.claude/projects"]
            min_block_chars = 0
        "#;
        assert_eq!(parse(src).unwrap_err().code(), ErrorCode::ConfigInvalid);
    }

    #[test]
    fn rejects_a_coverage_floor_outside_zero_to_one() {
        for bad in ["-0.1", "1.5"] {
            let src = format!(
                r#"
                [meta]
                harnex_version = ">=0.1, <0.2"
                [session]
                roots = ["~/.claude/projects"]
                coverage_floor = {bad}
                "#
            );
            assert_eq!(
                parse(&src).unwrap_err().code(),
                ErrorCode::ConfigInvalid,
                "coverage_floor={bad} must be rejected"
            );
        }
    }

    #[test]
    fn accepts_a_session_section_with_only_roots() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [session]
            roots = ["~/.claude/projects"]
        "#;
        let cfg = parse(src).unwrap();
        let session = cfg.session.unwrap();
        assert_eq!(session.min_block_chars, 40);
        assert_eq!(session.coverage_floor, 0.95);
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
            has_changes_check = ["git", "diff", "--quiet"]
        "#;
        assert!(parse(src).is_ok());
    }

    #[test]
    fn rejects_a_stop_audit_that_never_says_when_it_fires() {
        // Declaring the section commits to spawning a critique. Without the
        // probe the auditor answers "there is work" without looking, and every
        // Stop of every session costs a model call — the one field the section
        // needs and the only one that had no floor. A blank program name is
        // the same absence spelled differently; whether a named one is
        // installed is the runner's answer and not checked here.
        for probe in [
            "",
            r#"has_changes_check = []"#,
            r#"has_changes_check = [""]"#,
        ] {
            let src = format!(
                r#"
                [meta]
                harnex_version = ">=0.1, <0.2"
                [guard.stop_audit]
                critique_skill = "/critique"
                {probe}
                "#
            );
            assert_eq!(
                parse(&src).unwrap_err().code(),
                ErrorCode::ConfigInvalid,
                "probe {probe:?} must be refused"
            );
        }
        let named = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [guard.stop_audit]
            critique_skill = "/critique"
            has_changes_check = ["a-program-this-machine-may-not-have"]
        "#;
        assert!(parse(named).is_ok());
    }

    #[test]
    fn accepts_a_floor_with_directory_and_exact_protected_paths() {
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [guard.floor]
            protected_paths = ["hooks/", ".gitleaks.toml"]
        "#;
        assert!(parse(src).is_ok());
    }

    #[test]
    fn rejects_a_floor_entry_that_is_not_a_literal_relative_path() {
        for bad in ["../hooks/", "/etc/hooks", "hooks/*.sh", "", "/"] {
            let src = format!(
                r#"
                [meta]
                harnex_version = ">=0.1, <0.2"
                [guard.floor]
                protected_paths = ["{bad}"]
                "#
            );
            assert_eq!(
                parse(&src).unwrap_err().code(),
                ErrorCode::ConfigInvalid,
                "entry '{bad}' must be rejected"
            );
        }
    }

    #[test]
    fn rejects_a_floor_entry_restating_the_built_in_set_or_another_entry() {
        for paths in [
            r#"["harness.toml"]"#,
            r#"["harness.toml/"]"#,
            r#"[".claude/settings.json"]"#,
            r#"[".claude/Settings.local.json"]"#,
            r#"["hooks/", "Hooks/"]"#,
        ] {
            let src = format!(
                r#"
                [meta]
                harnex_version = ">=0.1, <0.2"
                [guard.floor]
                protected_paths = {paths}
                "#
            );
            assert_eq!(
                parse(&src).unwrap_err().code(),
                ErrorCode::ConfigInvalid,
                "{paths} must be rejected"
            );
        }
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
    fn rejects_codegen_target_path_traversal() {
        // A target path with `..` must be rejected at load — the runtime
        // write guard would reject it, so an otherwise-valid config that the
        // runtime cannot honor must not load (Article IV).
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"

            [codegen]
            [[codegen.groups]]
            name = "group-a"
            source = "source.toml"
            source_key = "values"
            [[codegen.groups.targets]]
            path = "../outside.md"
            begin = "<!-- BEGIN:x -->"
            end = "<!-- END:x -->"
            format = "markdown-bullet-list"
        "#;
        let err = parse(src).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid);
        assert!(err.to_string().contains("must be project-relative"));
    }

    #[test]
    fn rejects_codegen_target_absolute_path() {
        // An absolute target path wins a `join` and escapes working_dir, so it
        // must be rejected at load just like `..`.
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"

            [codegen]
            [[codegen.groups]]
            name = "group-a"
            source = "source.toml"
            source_key = "values"
            [[codegen.groups.targets]]
            path = "/etc/cron.d/evil.md"
            begin = "<!-- BEGIN:x -->"
            end = "<!-- END:x -->"
            format = "markdown-bullet-list"
        "#;
        let err = parse(src).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid);
        assert!(err.to_string().contains("must be project-relative"));
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
            name = "kx"
            [telemetry.kinds.payload_schema]
            type = "object"
            required = ["ok", 123]
            [telemetry.kinds.payload_schema.properties.ok]
            type = "string"
        "#;
        assert_eq!(parse(src).unwrap_err().code(), ErrorCode::ConfigInvalid);
    }

    #[test]
    fn rejects_telemetry_kind_name_with_path_separator() {
        // A telemetry kind name is used as a ledger filename, so a path
        // separator (or `..`) must be rejected at load — it would otherwise
        // escape the storage dir.
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [telemetry]
            storage = "jsonl"
            storage_dir = ".harness/telemetry"
            [[telemetry.kinds]]
            name = "../escape"
            [telemetry.kinds.payload_schema]
            type = "object"
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
            name = "kx"
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
    fn rejects_advisory_declarations_the_auditor_cannot_honor() {
        let base = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"

            [evidence]
            default_provenance = "internal"
            [[evidence.verifiers]]
            provenance = "internal"
            strategy = "file-path-line"
        "#;
        for (fragment, expect) in [
            (
                "[[evidence.advisories]]\nid = \"Bad_Id\"\ninputs = [\"src\"]\n",
                "not kebab-case",
            ),
            (
                "[[evidence.advisories]]\nid = \"a\"\ninputs = [\"src\"]\n[[evidence.advisories]]\nid = \"a\"\ninputs = [\"src\"]\n",
                "duplicate",
            ),
            (
                "[[evidence.advisories]]\nid = \"a\"\ninputs = []\n",
                "declares no inputs",
            ),
            (
                "[[evidence.advisories]]\nid = \"a\"\ninputs = [\"../out\"]\n",
                "literal project-relative",
            ),
            (
                "[[evidence.advisories]]\nid = \"a\"\ninputs = [\"src\"]\nengine = [\"src/*\"]\n",
                "literal project-relative",
            ),
        ] {
            let err = parse(&format!("{base}\n{fragment}")).unwrap_err();
            assert_eq!(err.code(), ErrorCode::ConfigInvalid, "{fragment}");
            assert!(err.to_string().contains(expect), "{fragment}: {err}");
        }
        let err = parse(
            r#"
            [meta]
            harnex_version = ">=0.1, <0.2"

            [evidence]
            default_provenance = "internal"
            advisory_dir = "/abs"
            [[evidence.verifiers]]
            provenance = "internal"
            strategy = "file-path-line"
        "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("advisory_dir"));
    }

    #[test]
    fn rejects_the_harvest_spelling_instead_of_ignoring_it() {
        let err = parse(
            r#"
            [meta]
            harnex_version = ">=0.1, <0.2"

            [evidence]
            default_provenance = "internal"
            [[evidence.verifiers]]
            provenance = "internal"
            strategy = "file-path-line"
            [[evidence.advisories]]
            id = "a"
            inputs = ["src"]
            unattendedRemeasure = true
        "#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("unattendedRemeasure"),
            "a key the schema does not know must fail loudly, never default the              declared behavior away: {err}"
        );
    }

    #[test]
    fn rejects_an_advisory_whose_input_contains_its_own_baseline() {
        for (dir, input) in [
            ("evidence", "evidence"),
            ("app/evidence", "app"),
            ("evidence", "evidence/a.json"),
        ] {
            let err = parse(&format!(
                r#"
                [meta]
                harnex_version = ">=0.1, <0.2"

                [evidence]
                default_provenance = "internal"
                advisory_dir = "{dir}"
                [[evidence.verifiers]]
                provenance = "internal"
                strategy = "file-path-line"
                [[evidence.advisories]]
                id = "a"
                inputs = ["{input}"]
            "#
            ))
            .unwrap_err();
            assert!(
                err.to_string().contains("its own basis"),
                "{dir}/{input}: {err}"
            );
        }
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
    fn rejects_an_extra_rule_no_permission_check_consults() {
        for (field, rule) in [
            ("extra_deny", "Write(/secrets/**)"),
            ("extra_deny", "Bash(command:rm -rf *)"),
            ("extra_ask", "NotebookEdit(notebooks/**)"),
            ("extra_allow", "Glob(src/**)"),
        ] {
            let src = format!(
                r#"
                [meta]
                harnex_version = ">=0.1, <0.2"
                [policy.permissions]
                profiles = ["baseline"]
                {field} = ["{rule}"]
            "#
            );
            let err = parse(&src).unwrap_err();
            assert_eq!(
                err.code(),
                ErrorCode::PolicyRuleInert,
                "{field} = {rule} must fail at load"
            );
            assert!(err.hint().is_some(), "{rule} must say what to write");
        }
    }

    #[test]
    fn accepts_extra_rules_a_permission_check_reads() {
        // `Bash(find * -delete)` places text after a wildcard, which on the
        // deny side is the sanctioned over-reach, not a trap.
        let src = r#"
            [meta]
            harnex_version = ">=0.1, <0.2"
            [policy.permissions]
            profiles = ["baseline"]
            extra_deny = ["Edit(/vault/**)", "Bash(terraform apply *)", "Agent(model:opus)", "Bash(find * -delete)"]
            extra_allow = ["Bash(pnpm gate *)", "Write"]
        "#;
        parse(src).unwrap();
    }

    #[test]
    fn rejects_extra_rules_that_reach_other_than_they_read() {
        for (field, rule) in [
            ("extra_allow", "Bash(pnpm gate:*)"),
            ("extra_deny", "Bash(git push:*)"),
            ("extra_allow", "Bash(pnpm --filter * dev)"),
        ] {
            let src = format!(
                r#"
                [meta]
                harnex_version = ">=0.1, <0.2"
                [policy.permissions]
                profiles = ["baseline"]
                {field} = ["{rule}"]
            "#
            );
            let err = parse(&src).unwrap_err();
            assert_eq!(
                err.code(),
                ErrorCode::PolicyRuleMisleading,
                "{field} = {rule} must fail at load"
            );
            assert!(err.hint().is_some(), "{rule} must say what to write");
        }
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
