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

pub type Result<T> = std::result::Result<T, Error>;

/// Stable, kebab-screaming-snake error codes that appear in the JSON
/// envelope `error.code` field. Mapping is a public contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    ConfigInvalid,
    ConfigNotFound,
    ConfigVersionMismatch,
    PathTraversal,
    PathSymlinkRefused,
    IoFailure,
    TelemetryKindUnknown,
    TelemetryPayloadInvalid,
    CodegenSourceMissing,
    CodegenSourceKeyMissing,
    CodegenSourceShapeInvalid,
    CodegenRendererUnknown,
    CodegenSentinelMissing,
    CodegenCycle,
    PolicyProfileUnknown,
    PolicyVersionFailure,
    ValidateFrontmatterMalformed,
    ValidateFrontmatterInvalid,
    LifecycleObservationCorrupt,
    LifecycleConsumerStrategyUnknown,
    LifecycleDemoteWithoutApproval,
    LifecycleDecisionTextEmpty,
    GuardHookInputInvalid,
    GuardSpawnFailure,
    GraphResponseInvalid,
    GraphSpawnFailure,
    CheckGitFailure,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfigInvalid => "CONFIG_INVALID",
            Self::ConfigNotFound => "CONFIG_NOT_FOUND",
            Self::ConfigVersionMismatch => "CONFIG_VERSION_MISMATCH",
            Self::PathTraversal => "PATH_TRAVERSAL",
            Self::PathSymlinkRefused => "PATH_SYMLINK_REFUSED",
            Self::IoFailure => "IO_FAILURE",
            Self::TelemetryKindUnknown => "TELEMETRY_KIND_UNKNOWN",
            Self::TelemetryPayloadInvalid => "TELEMETRY_PAYLOAD_INVALID",
            Self::CodegenSourceMissing => "CODEGEN_SOURCE_MISSING",
            Self::CodegenSourceKeyMissing => "CODEGEN_SOURCE_KEY_MISSING",
            Self::CodegenSourceShapeInvalid => "CODEGEN_SOURCE_SHAPE_INVALID",
            Self::CodegenRendererUnknown => "CODEGEN_RENDERER_UNKNOWN",
            Self::CodegenSentinelMissing => "CODEGEN_SENTINEL_MISSING",
            Self::CodegenCycle => "CODEGEN_CYCLE",
            Self::PolicyProfileUnknown => "POLICY_PROFILE_UNKNOWN",
            Self::PolicyVersionFailure => "POLICY_VERSION_FAILURE",
            Self::ValidateFrontmatterMalformed => "VALIDATE_FRONTMATTER_MALFORMED",
            Self::ValidateFrontmatterInvalid => "VALIDATE_FRONTMATTER_INVALID",
            Self::LifecycleObservationCorrupt => "LIFECYCLE_OBSERVATION_CORRUPT",
            Self::LifecycleConsumerStrategyUnknown => "LIFECYCLE_CONSUMER_STRATEGY_UNKNOWN",
            Self::LifecycleDemoteWithoutApproval => "LIFECYCLE_DEMOTE_WITHOUT_APPROVAL",
            Self::LifecycleDecisionTextEmpty => "LIFECYCLE_DECISION_TEXT_EMPTY",
            Self::GuardHookInputInvalid => "GUARD_HOOK_INPUT_INVALID",
            Self::GuardSpawnFailure => "GUARD_SPAWN_FAILURE",
            Self::GraphResponseInvalid => "GRAPH_RESPONSE_INVALID",
            Self::GraphSpawnFailure => "GRAPH_SPAWN_FAILURE",
            Self::CheckGitFailure => "CHECK_GIT_FAILURE",
        }
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

    #[error("codegen cycle: target {path:?} is also a source")]
    CodegenCycle { path: PathBuf },

    #[error("policy profile unknown: '{name}'")]
    PolicyProfileUnknown { name: String },

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
            Self::CodegenCycle { .. } => ErrorCode::CodegenCycle,
            Self::PolicyProfileUnknown { .. } => ErrorCode::PolicyProfileUnknown,
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
            Self::GuardHookInputInvalid { .. } => ErrorCode::GuardHookInputInvalid,
            Self::GuardSpawnFailure { .. } => ErrorCode::GuardSpawnFailure,
            Self::GraphResponseInvalid { .. } => ErrorCode::GraphResponseInvalid,
            Self::GraphSpawnFailure { .. } => ErrorCode::GraphSpawnFailure,
            Self::CheckGitFailure { .. } => ErrorCode::CheckGitFailure,
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
            Self::CodegenCycle { .. } => Some("targets must not be source files in any group"),
            Self::PolicyProfileUnknown { .. } => Some(
                "use one of the built-in profiles or register a custom profile in harness.toml",
            ),
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
            Self::LifecycleDemoteWithoutApproval { .. } => Some(
                "demote applies only to previously Approved patterns; use reject for never-approved patterns",
            ),
            Self::LifecycleDecisionTextEmpty => Some(
                "pass a non-empty --decision-text; the toolkit refuses to invent promotion rationale",
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
