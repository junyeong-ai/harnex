//! # Typed errors with stable codes
//!
//! Every failure in the toolkit is a variant of [`Error`]. The
//! `ErrorCode::as_str` mapping is a stable public contract — changing
//! a variant's string form is a MAJOR version bump.
//!
//! ## What this module refuses to do
//!
//! - Never use string matching to identify errors. Consumers pattern-match
//!   on [`ErrorCode`] or downcast via [`Error::code`].
//! - Never bury an io error without a path. Every IO failure carries the
//!   exact path that triggered it.

use std::path::PathBuf;

use thiserror::Error as ThisError;

use crate::envelope::Location;
use crate::wire_enum::wire_enum;

pub type Result<T> = std::result::Result<T, Error>;

wire_enum! {
    /// Stable, kebab-screaming-snake error codes that appear in the JSON
    /// envelope `error.code` field. Mapping is a public contract.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ErrorCode {
        ConfigInvalid => "CONFIG_INVALID",
        ConfigNotFound => "CONFIG_NOT_FOUND",
        ConfigVersionMismatch => "CONFIG_VERSION_MISMATCH",
        PathTraversal => "PATH_TRAVERSAL",
        PathSymlinkRefused => "PATH_SYMLINK_REFUSED",
        IoFailure => "IO_FAILURE",
        TelemetryKindUnknown => "TELEMETRY_KIND_UNKNOWN",
        TelemetryPayloadInvalid => "TELEMETRY_PAYLOAD_INVALID",
        CodegenSourceMissing => "CODEGEN_SOURCE_MISSING",
        CodegenSourceKeyMissing => "CODEGEN_SOURCE_KEY_MISSING",
        CodegenSourceShapeInvalid => "CODEGEN_SOURCE_SHAPE_INVALID",
        CodegenRendererUnknown => "CODEGEN_RENDERER_UNKNOWN",
        CodegenSentinelMissing => "CODEGEN_SENTINEL_MISSING",
        CodegenSentinelDuplicate => "CODEGEN_SENTINEL_DUPLICATE",
        CodegenCycle => "CODEGEN_CYCLE",
        PolicyProfileUnknown => "POLICY_PROFILE_UNKNOWN",
        PolicyRuleInert => "POLICY_RULE_INERT",
        PolicyRuleMisleading => "POLICY_RULE_MISLEADING",
        PolicyVersionFailure => "POLICY_VERSION_FAILURE",
        ValidateFrontmatterMalformed => "VALIDATE_FRONTMATTER_MALFORMED",
        ValidateFrontmatterInvalid => "VALIDATE_FRONTMATTER_INVALID",
        LifecycleObservationCorrupt => "LIFECYCLE_OBSERVATION_CORRUPT",
        LifecycleConsumerStrategyUnknown => "LIFECYCLE_CONSUMER_STRATEGY_UNKNOWN",
        LifecycleDemoteWithoutApproval => "LIFECYCLE_DEMOTE_WITHOUT_APPROVAL",
        LifecycleDecisionTextEmpty => "LIFECYCLE_DECISION_TEXT_EMPTY",
        LifecycleTagEmpty => "LIFECYCLE_TAG_EMPTY",
        GuardHookInputInvalid => "GUARD_HOOK_INPUT_INVALID",
        GuardSpawnFailure => "GUARD_SPAWN_FAILURE",
        GraphResponseInvalid => "GRAPH_RESPONSE_INVALID",
        GraphSpawnFailure => "GRAPH_SPAWN_FAILURE",
        CheckGitFailure => "CHECK_GIT_FAILURE",
        LifecycleGitFailure => "LIFECYCLE_GIT_FAILURE",
        GovernsQueryUnresolvable => "GOVERNS_QUERY_UNRESOLVABLE",
        SessionRootUnreadable => "SESSION_ROOT_UNREADABLE",
        SessionCoverageBelowFloor => "SESSION_COVERAGE_BELOW_FLOOR",
        SessionWindowUnattributed => "SESSION_WINDOW_UNATTRIBUTED",
        SessionBaselineLabelRejected => "SESSION_BASELINE_LABEL_REJECTED",
        SessionBaselineNotComparable => "SESSION_BASELINE_NOT_COMPARABLE",
        SessionBaselineUnreadable => "SESSION_BASELINE_UNREADABLE",
    }
}

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("config invalid: {message}")]
    ConfigInvalid {
        message: String,
        location: Option<Location>,
    },

    #[error("harness.toml not found from {path:?} upward")]
    ConfigNotFound { path: PathBuf },

    #[error("config requires harnex {required}, this binary is {actual}")]
    ConfigVersionMismatch { required: String, actual: String },

    #[error("path traversal refused: {path:?}")]
    PathTraversal { path: PathBuf },

    #[error("refusing to write through symlink: {path:?}")]
    PathSymlinkRefused { path: PathBuf },

    #[error("io failure on {path:?}: {source}")]
    IoFailure {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("unknown telemetry kind: {kind}")]
    TelemetryKindUnknown { kind: String },

    #[error("telemetry payload invalid: {message}")]
    TelemetryPayloadInvalid { message: String },

    #[error("codegen source file missing: {path:?}")]
    CodegenSourceMissing { path: PathBuf },

    #[error("codegen source key '{key}' not found in {path:?}")]
    CodegenSourceKeyMissing { key: String, path: PathBuf },

    #[error("codegen source key '{key}' is not an array of strings in {path:?}")]
    CodegenSourceShapeInvalid { key: String, path: PathBuf },

    #[error("codegen renderer unknown: '{name}'")]
    CodegenRendererUnknown { name: String },

    #[error("codegen sentinel '{begin}' / '{end}' not found in {path:?}")]
    CodegenSentinelMissing {
        begin: String,
        end: String,
        path: PathBuf,
    },

    #[error("codegen sentinel '{begin}' / '{end}' appears more than once in {path:?}")]
    CodegenSentinelDuplicate {
        begin: String,
        end: String,
        path: PathBuf,
    },

    #[error("codegen cycle: target {path:?} is also a source")]
    CodegenCycle { path: PathBuf },

    #[error("policy profile unknown: '{name}'")]
    PolicyProfileUnknown { name: String },

    #[error("permission rule {rule:?} in {field} is never consulted: {reason}")]
    PolicyRuleInert {
        field: &'static str,
        rule: String,
        reason: String,
        hint: String,
    },

    #[error("permission rule {rule:?} in {field} reaches other than it reads: {reason}")]
    PolicyRuleMisleading {
        field: &'static str,
        rule: String,
        reason: String,
        hint: String,
    },

    #[error("policy version check failed: {message}")]
    PolicyVersionFailure { message: String },

    #[error("frontmatter malformed in {path:?}: {message}")]
    ValidateFrontmatterMalformed { path: PathBuf, message: String },

    #[error("frontmatter invalid in {path:?}: {message}")]
    ValidateFrontmatterInvalid { path: PathBuf, message: String },

    #[error("observation ledger corrupt at {path:?}: {message}")]
    LifecycleObservationCorrupt { path: PathBuf, message: String },

    #[error("consumer detector strategy unknown: '{strategy}'")]
    LifecycleConsumerStrategyUnknown { strategy: String },

    #[error("demote refused: no prior Approved decision for ({tag}, {normalized_text})")]
    LifecycleDemoteWithoutApproval {
        tag: String,
        normalized_text: String,
    },

    #[error("decision_text is empty — promotion requires human-authored rationale")]
    LifecycleDecisionTextEmpty,

    #[error("tag is empty — a ledger record is grouped by its tag")]
    LifecycleTagEmpty,

    #[error("guard hook input invalid: {message}")]
    GuardHookInputInvalid { message: String },

    #[error("guard spawn failure: {message}")]
    GuardSpawnFailure { message: String },

    #[error("graph response invalid: {message}")]
    GraphResponseInvalid { message: String },

    #[error("nodex spawn failure: {message}")]
    GraphSpawnFailure { message: String },

    #[error("git command failed: {message}")]
    CheckGitFailure { message: String },

    #[error("git command failed: {message}")]
    LifecycleGitFailure { message: String },

    #[error("governs query cannot resolve inside the project: '{query}'")]
    GovernsQueryUnresolvable { query: String },

    #[error("session root {path} unreadable: {message}")]
    SessionRootUnreadable { path: PathBuf, message: String },

    #[error("session coverage {observed:.3} is below the configured floor {floor:.3}: {message}")]
    SessionCoverageBelowFloor {
        observed: f64,
        floor: f64,
        message: String,
    },

    /// A window with nothing to measure, which is not a low measurement. It
    /// carries no ratio because none was taken: reporting the absence as `0.000
    /// below a floor of 0.000` states a measurement and a comparison that never
    /// happened, and sends the reader to a floor that was not the reason.
    #[error("no turn in the window was attributed to a person: {message}")]
    SessionWindowUnattributed { message: String },

    #[error("baseline label '{label}' rejected: {message}")]
    SessionBaselineLabelRejected { label: String, message: String },

    #[error("baselines cannot be compared: {message}")]
    SessionBaselineNotComparable { message: String },

    #[error("{path}: {message}")]
    SessionBaselineUnreadable { path: PathBuf, message: String },
}

impl Error {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::ConfigInvalid { .. } => ErrorCode::ConfigInvalid,
            Self::ConfigNotFound { .. } => ErrorCode::ConfigNotFound,
            Self::ConfigVersionMismatch { .. } => ErrorCode::ConfigVersionMismatch,
            Self::PathTraversal { .. } => ErrorCode::PathTraversal,
            Self::PathSymlinkRefused { .. } => ErrorCode::PathSymlinkRefused,
            Self::IoFailure { .. } => ErrorCode::IoFailure,
            Self::TelemetryKindUnknown { .. } => ErrorCode::TelemetryKindUnknown,
            Self::TelemetryPayloadInvalid { .. } => ErrorCode::TelemetryPayloadInvalid,
            Self::CodegenSourceMissing { .. } => ErrorCode::CodegenSourceMissing,
            Self::CodegenSourceKeyMissing { .. } => ErrorCode::CodegenSourceKeyMissing,
            Self::CodegenSourceShapeInvalid { .. } => ErrorCode::CodegenSourceShapeInvalid,
            Self::CodegenRendererUnknown { .. } => ErrorCode::CodegenRendererUnknown,
            Self::CodegenSentinelMissing { .. } => ErrorCode::CodegenSentinelMissing,
            Self::CodegenSentinelDuplicate { .. } => ErrorCode::CodegenSentinelDuplicate,
            Self::CodegenCycle { .. } => ErrorCode::CodegenCycle,
            Self::PolicyProfileUnknown { .. } => ErrorCode::PolicyProfileUnknown,
            Self::PolicyRuleInert { .. } => ErrorCode::PolicyRuleInert,
            Self::PolicyRuleMisleading { .. } => ErrorCode::PolicyRuleMisleading,
            Self::PolicyVersionFailure { .. } => ErrorCode::PolicyVersionFailure,
            Self::ValidateFrontmatterMalformed { .. } => ErrorCode::ValidateFrontmatterMalformed,
            Self::ValidateFrontmatterInvalid { .. } => ErrorCode::ValidateFrontmatterInvalid,
            Self::LifecycleObservationCorrupt { .. } => ErrorCode::LifecycleObservationCorrupt,
            Self::LifecycleConsumerStrategyUnknown { .. } => {
                ErrorCode::LifecycleConsumerStrategyUnknown
            }
            Self::LifecycleDemoteWithoutApproval { .. } => {
                ErrorCode::LifecycleDemoteWithoutApproval
            }
            Self::LifecycleDecisionTextEmpty => ErrorCode::LifecycleDecisionTextEmpty,
            Self::LifecycleTagEmpty => ErrorCode::LifecycleTagEmpty,
            Self::GuardHookInputInvalid { .. } => ErrorCode::GuardHookInputInvalid,
            Self::GuardSpawnFailure { .. } => ErrorCode::GuardSpawnFailure,
            Self::GraphResponseInvalid { .. } => ErrorCode::GraphResponseInvalid,
            Self::GraphSpawnFailure { .. } => ErrorCode::GraphSpawnFailure,
            Self::CheckGitFailure { .. } => ErrorCode::CheckGitFailure,
            Self::LifecycleGitFailure { .. } => ErrorCode::LifecycleGitFailure,
            Self::GovernsQueryUnresolvable { .. } => ErrorCode::GovernsQueryUnresolvable,
            Self::SessionRootUnreadable { .. } => ErrorCode::SessionRootUnreadable,
            Self::SessionCoverageBelowFloor { .. } => ErrorCode::SessionCoverageBelowFloor,
            Self::SessionWindowUnattributed { .. } => ErrorCode::SessionWindowUnattributed,
            Self::SessionBaselineLabelRejected { .. } => ErrorCode::SessionBaselineLabelRejected,
            Self::SessionBaselineNotComparable { .. } => ErrorCode::SessionBaselineNotComparable,
            Self::SessionBaselineUnreadable { .. } => ErrorCode::SessionBaselineUnreadable,
        }
    }

    pub fn hint(&self) -> Option<&str> {
        match self {
            Self::ConfigNotFound { .. } => {
                Some("create harness.toml at the project root (see examples/)")
            }
            Self::ConfigVersionMismatch { .. } => {
                Some("update harness.toml [meta] harnex_version or upgrade the binary")
            }
            Self::PathTraversal { .. } => Some("paths must not contain '..' segments"),
            Self::GovernsQueryUnresolvable { .. } => {
                Some("pass a project-relative path, or an absolute path under the project root")
            }
            Self::PathSymlinkRefused { .. } => {
                Some("delete the symlink or write to a non-symlink path")
            }
            Self::TelemetryKindUnknown { .. } => {
                Some("declare the kind under [[telemetry.kinds]] in harness.toml")
            }
            Self::TelemetryPayloadInvalid { .. } => {
                Some("adjust payload to match the kind's payload_schema")
            }
            Self::CodegenSourceMissing { .. } => {
                Some("create the source file or correct the source path")
            }
            Self::CodegenSourceKeyMissing { .. } => Some("check the dot-path in source_key"),
            Self::CodegenSourceShapeInvalid { .. } => {
                Some("source value must be a TOML array of strings")
            }
            Self::CodegenRendererUnknown { .. } => Some(
                "use one of: toml-array-assignment, bash-array-assignment, markdown-bullet-list",
            ),
            Self::CodegenSentinelMissing { .. } => {
                Some("add the BEGIN/END sentinel lines to the target file")
            }
            Self::CodegenSentinelDuplicate { .. } => Some(
                "the target file must contain exactly one BEGIN/END sentinel pair — remove the duplicate",
            ),
            Self::CodegenCycle { .. } => Some("targets must not be source files in any group"),
            Self::PolicyProfileUnknown { .. } => Some(
                "use one of the built-in profiles or register a custom profile in harness.toml",
            ),
            Self::PolicyRuleInert { hint, .. } => Some(hint),
            Self::PolicyRuleMisleading { hint, .. } => Some(hint),
            Self::ValidateFrontmatterMalformed { .. } => {
                Some("frontmatter must be `---`-delimited YAML at the top of the file")
            }
            Self::GuardHookInputInvalid { .. } => {
                Some("hook stdin must be JSON matching the Claude Code event schema")
            }
            Self::GraphResponseInvalid { .. } => {
                Some("check nodex output format — expected a JSON envelope")
            }
            Self::GraphSpawnFailure { .. } => Some("ensure nodex is installed and on PATH"),
            Self::CheckGitFailure { .. } => {
                Some("ensure git is installed and the working directory is a repository")
            }
            Self::LifecycleGitFailure { .. } => Some(
                "the grep consumer detector reads the files git says the project owns; ensure git is installed and the working directory is a repository",
            ),
            Self::LifecycleDemoteWithoutApproval { .. } => Some(
                "demote applies only to previously Approved patterns; use reject for never-approved patterns",
            ),
            Self::LifecycleDecisionTextEmpty => Some(
                "pass a non-empty --decision-text; the toolkit refuses to invent promotion rationale",
            ),
            Self::LifecycleTagEmpty => {
                Some("pass a non-empty --tag naming the topic this record groups under")
            }
            Self::SessionRootUnreadable { .. } => Some(
                "check [session] roots in harness.toml: every root must name a readable directory",
            ),
            Self::SessionCoverageBelowFloor { .. } => Some(
                "widen the window, or lower [session] coverage_floor once you accept the bias it admits",
            ),
            Self::SessionWindowUnattributed { .. } => Some(
                "widen the window with --since: `baseline save` starts where the last window of the same scope ended, and nothing was asked of this project since then",
            ),
            Self::SessionBaselineLabelRejected { .. } => Some(
                "choose a label no earlier baseline used; the ledger is append-only and a label names one window",
            ),
            Self::SessionBaselineNotComparable { .. } => Some(
                "a comparison needs two recorded windows of the same scope that do not overlap; `baseline save` starts where the last window of that scope ended",
            ),
            Self::SessionBaselineUnreadable { .. } => Some(
                "the line named is not a window this build can place: move the ledger aside and record the next window from here",
            ),
            _ => None,
        }
    }

    pub fn location(&self) -> Option<&Location> {
        match self {
            Self::ConfigInvalid { location, .. } => location.as_ref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod error_code_tests {
    use super::ErrorCode;
    use std::collections::BTreeSet;

    #[test]
    fn all_codes_have_unique_nonempty_strings() {
        let mut seen = BTreeSet::new();
        for code in ErrorCode::ALL {
            let s = code.as_str();
            assert!(!s.is_empty(), "{code:?} has empty as_str");
            assert!(seen.insert(s), "duplicate code string: {s}");
        }
    }

    #[test]
    fn all_entries_are_unique_and_nonshrinking() {
        // Guards what a test CAN guard without variant reflection: ALL carries
        // no duplicate (a copy-paste slip) and never shrinks (a dropped
        // variant). It does NOT prove ALL lists every variant — on stable Rust
        // that needs variant reflection. The defense against an as_str-present
        // but ALL-absent variant is procedural: the exhaustive `as_str` match
        // forces editing THIS file when a variant is added (ALL sits directly
        // above it), and the add-variant checklist in crates/harness-core/
        // CLAUDE.md names the step. Bump the floor when a variant is added.
        let count = ErrorCode::ALL.len();
        let unique: BTreeSet<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(count, unique.len(), "ALL has a duplicate variant");
        assert!(count >= 38, "ALL shrank unexpectedly — variant dropped?");
    }
}
