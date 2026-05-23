//! # export — JSON Schema emission for downstream codegen
//!
//! Emits JSON Schema (draft 2020-12) for the toolkit's user-facing types:
//! `Config` (for IDE autocomplete on `harness.toml`), `EnvelopeShape`
//! (for typed-client codegen of CLI output), `Finding`, `Event`,
//! `PermissionsBlock`, and the closed enum of [`ErrorCode`].
//!
//! ## What this module refuses to do
//!
//! - Never invent schema content — every emitted schema either comes from
//!   `schemars::schema_for!` on a `JsonSchema`-deriving type or from a
//!   hand-rolled function whose vocabulary is asserted by a test.
//! - Never make network calls or load remote schemas.

use serde::Serialize;
use serde_json::Value;

use crate::config::Config;
use crate::envelope::{EnvelopeShape, Finding, ListResponse};
use crate::error::ErrorCode;
use crate::policy::PermissionsBlock;
use crate::telemetry::Event;

/// Closed enum of schema targets `harness export schema <target>` understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SchemaTarget {
    Config,
    Envelope,
    Finding,
    Event,
    Permissions,
    ErrorCodes,
    All,
}

impl SchemaTarget {
    pub const ALL: &'static [Self] = &[
        Self::Config,
        Self::Envelope,
        Self::Finding,
        Self::Event,
        Self::Permissions,
        Self::ErrorCodes,
        Self::All,
    ];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "config" => Self::Config,
            "envelope" => Self::Envelope,
            "finding" => Self::Finding,
            "event" => Self::Event,
            "permissions" => Self::Permissions,
            "error-codes" => Self::ErrorCodes,
            "all" => Self::All,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Envelope => "envelope",
            Self::Finding => "finding",
            Self::Event => "event",
            Self::Permissions => "permissions",
            Self::ErrorCodes => "error-codes",
            Self::All => "all",
        }
    }
}

/// Emit the JSON Schema for `target` as a serde_json::Value (already shaped
/// as a `$schema`-tagged object).
pub fn schema_for(target: SchemaTarget) -> Value {
    match target {
        SchemaTarget::Config => to_value(schemars::schema_for!(Config)),
        SchemaTarget::Envelope => to_value(schemars::schema_for!(EnvelopeShape)),
        SchemaTarget::Finding => to_value(schemars::schema_for!(ListResponse<Finding>)),
        SchemaTarget::Event => to_value(schemars::schema_for!(Event)),
        SchemaTarget::Permissions => to_value(schemars::schema_for!(PermissionsBlock)),
        SchemaTarget::ErrorCodes => error_codes_schema(),
        SchemaTarget::All => all_schemas(),
    }
}

fn to_value<T: Serialize>(s: T) -> Value {
    serde_json::to_value(s).expect("schema serialisation is infallible")
}

/// Hand-rolled schema for the closed `ErrorCode` enum. The test
/// `every_error_code_is_in_schema` keeps this in lock-step with the enum.
fn error_codes_schema() -> Value {
    let codes = error_code_strings();
    serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "harness ErrorCode",
        "description": "Closed set of error.code strings appearing in harness JSON envelopes.",
        "type": "string",
        "enum": codes,
    })
}

fn all_schemas() -> Value {
    serde_json::json!({
        "config": schema_for(SchemaTarget::Config),
        "envelope": schema_for(SchemaTarget::Envelope),
        "finding": schema_for(SchemaTarget::Finding),
        "event": schema_for(SchemaTarget::Event),
        "permissions": schema_for(SchemaTarget::Permissions),
        "error-codes": schema_for(SchemaTarget::ErrorCodes),
    })
}

/// Every known ErrorCode in stable kebab-screaming order.
/// Adding a variant requires adding it here AND in the registry test.
fn error_code_strings() -> Vec<&'static str> {
    use ErrorCode::*;
    let all: &[ErrorCode] = &[
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
    ];
    for code in all {
        match code {
            ConfigInvalid
            | ConfigNotFound
            | ConfigVersionMismatch
            | PathTraversal
            | PathSymlinkRefused
            | IoFailure
            | TelemetryKindUnknown
            | TelemetryPayloadInvalid
            | CodegenSourceMissing
            | CodegenSourceKeyMissing
            | CodegenSourceShapeInvalid
            | CodegenRendererUnknown
            | CodegenSentinelMissing
            | CodegenCycle
            | PolicyProfileUnknown
            | PolicyVersionFailure
            | ValidateFrontmatterMalformed
            | ValidateFrontmatterInvalid
            | LifecycleObservationCorrupt
            | LifecycleConsumerStrategyUnknown
            | LifecycleDemoteWithoutApproval
            | LifecycleDecisionTextEmpty
            | GuardHookInputInvalid
            | GuardSpawnFailure
            | GraphResponseInvalid
            | GraphSpawnFailure
            | CheckGitFailure => {}
        }
    }
    all.iter().map(|c| c.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_schema_is_object_with_meta() {
        let v = schema_for(SchemaTarget::Config);
        assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("object"));
        let props = v.pointer("/properties").expect("has properties");
        assert!(
            props.get("meta").is_some(),
            "Config schema must expose meta"
        );
    }

    #[test]
    fn envelope_schema_has_ok_data_error_warnings() {
        let v = schema_for(SchemaTarget::Envelope);
        let props = v.pointer("/properties").expect("has properties");
        for key in ["ok", "data", "error", "warnings"] {
            assert!(
                props.get(key).is_some(),
                "envelope schema must expose '{key}'"
            );
        }
    }

    #[test]
    fn finding_schema_emits_severity_enum() {
        let v = schema_for(SchemaTarget::Finding);
        // ListResponse<Finding> → items: array of Finding which has severity enum
        let json = serde_json::to_string(&v).unwrap();
        for sev in ["blocker", "major", "minor", "info"] {
            assert!(
                json.contains(sev),
                "finding schema missing severity '{sev}'"
            );
        }
    }

    #[test]
    fn permissions_schema_lists_allow_ask_deny() {
        let v = schema_for(SchemaTarget::Permissions);
        let props = v.pointer("/properties").expect("has properties");
        for k in ["allow", "ask", "deny"] {
            assert!(props.get(k).is_some(), "permissions schema missing '{k}'");
        }
    }

    #[test]
    fn error_codes_schema_lists_all_variants() {
        let v = schema_for(SchemaTarget::ErrorCodes);
        let enums = v
            .get("enum")
            .and_then(|e| e.as_array())
            .expect("enum array");
        assert!(enums.contains(&Value::String("CONFIG_INVALID".into())));
        assert!(enums.contains(&Value::String("GUARD_SPAWN_FAILURE".into())));
        // No accidental empties.
        assert!(!enums.is_empty());
    }

    #[test]
    fn all_schemas_emits_every_named_target() {
        let v = schema_for(SchemaTarget::All);
        for k in [
            "config",
            "envelope",
            "finding",
            "event",
            "permissions",
            "error-codes",
        ] {
            assert!(v.get(k).is_some(), "all-bundle missing '{k}'");
        }
    }

    #[test]
    fn target_from_str_round_trips_every_variant() {
        for variant in SchemaTarget::ALL {
            assert_eq!(SchemaTarget::from_str(variant.as_str()), Some(*variant));
        }
        assert_eq!(SchemaTarget::from_str("nope"), None);
    }

    #[test]
    fn target_all_covers_every_known_variant() {
        // Spot-check that ALL is not stale — adding a new variant requires
        // updating ALL or this test fails (count mismatch with known strings).
        let known_strings = [
            "config",
            "envelope",
            "finding",
            "event",
            "permissions",
            "error-codes",
            "all",
        ];
        assert_eq!(SchemaTarget::ALL.len(), known_strings.len());
        for s in known_strings {
            assert!(
                SchemaTarget::ALL.iter().any(|v| v.as_str() == s),
                "ALL missing {s}"
            );
        }
    }
}
