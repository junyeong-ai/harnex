//! # JSON envelope contract
//!
//! Every CLI command emits exactly one envelope on stdout:
//!
//! - Success: `{"ok": true, "data": T, "warnings": [...]}`
//! - Error:   `{"ok": false, "error": {"code", "message", "hint?", "location?"}}`
//!
//! List-shaped responses use [`ListResponse`] for `data`, which carries
//! `items` + `total` + an explicit `skipped_rules` list. A consumer who
//! sees `skipped_rules.len() > 0` knows the absence of findings does NOT
//! imply the absent rules passed — they did not run.
//!
//! ## What this module refuses to do
//!
//! - Never accept plain text on stdout. Helpers route through `serde_json`.
//! - Never silently emit pretty-printed JSON in production paths. The
//!   envelope is one line per call.

use std::io::{self, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Structured location of a finding or error.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Location {
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,
}

impl Location {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            line: None,
            col: None,
        }
    }
    pub fn line(path: impl Into<PathBuf>, line: u32) -> Self {
        Self {
            path: path.into(),
            line: Some(line),
            col: None,
        }
    }
}

/// Non-blocking warning attached to a successful envelope.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Warning {
    pub code: String,
    pub message: String,
}

/// Severity ladder. Closed enum, kebab-case in JSON.
///
/// `Blocker` and `Major` are the GATING tiers: any finding at or above
/// `Major` fails the validation gate (non-zero exit, blocks commits / CI).
/// `Minor` and `Info` are advisory — surfaced in the envelope but never
/// change the exit code (e.g. an opted-out memory-only claim is `Minor`).
/// New tiers may be added at the top when a class of findings needs to
/// outrank Blocker; doing so requires updating `Severity::rank` (the
/// compiler enforces the exhaustive `match`). The gate threshold lives in
/// one place — [`Severity::fails_gate`] — consumed by every CLI command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Blocker,
    Major,
    Minor,
    Info,
}

impl Severity {
    pub fn rank(self) -> u8 {
        match self {
            Self::Blocker => 0,
            Self::Major => 1,
            Self::Minor => 2,
            Self::Info => 3,
        }
    }

    /// Whether a finding of this severity fails the validation gate (exit 1).
    /// Single source of truth for the gate threshold: `Blocker | Major`.
    /// `Minor` / `Info` are advisory and do not change the exit code.
    pub fn fails_gate(self) -> bool {
        self.rank() <= Self::Major.rank()
    }
}

#[cfg(test)]
mod success_shape_tests {
    use super::{Warning, write_success};

    #[test]
    fn warnings_is_present_even_when_empty() {
        // Article II states the success shape as `{ok, data?, error?,
        // warnings[]}`. Omitting the key in the quiet case makes every
        // consumer special-case the common path.
        let mut out = Vec::new();
        write_success(&mut out, serde_json::json!({"n": 1}), &[]).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(
            value.get("warnings"),
            Some(&serde_json::json!([])),
            "{value}"
        );
    }

    #[test]
    fn warnings_carry_code_and_message() {
        let mut out = Vec::new();
        let warnings = vec![Warning {
            code: "w".into(),
            message: "m".into(),
        }];
        write_success(&mut out, serde_json::json!({}), &warnings).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["warnings"][0]["code"], "w");
        assert_eq!(value["warnings"][0]["message"], "m");
    }
}

#[cfg(test)]
mod severity_tests {
    use super::Severity;

    #[test]
    fn gate_threshold_is_blocker_and_major() {
        assert!(Severity::Blocker.fails_gate());
        assert!(Severity::Major.fails_gate());
        assert!(!Severity::Minor.fails_gate(), "Minor is advisory");
        assert!(!Severity::Info.fails_gate(), "Info is advisory");
    }
}

/// Declares [`FixCommand`]: its variants and their wire strings, together.
///
/// A hand-written `ALL` beside an exhaustive `as_str` is two representations of
/// one fact, and the compiler only forces the second — a variant missing from
/// `ALL` serialised through `as_str` while the schema built from `ALL` did not
/// describe it and `Deserialize` rejected the value the binary had just
/// emitted, with nothing failing. Generating both from one declaration removes
/// that state rather than testing for it.
macro_rules! fix_commands {
    ($($variant:ident => $wire:literal),+ $(,)?) => {
        /// Closed set of auto-fix commands the safe-fix registry recognises.
        ///
        /// Lives in the envelope because it is the value of a [`Finding`]
        /// field: making the field `Option<FixCommand>` is what keeps an emit
        /// site from inventing a command, which prose asking for
        /// `FixCommand::X.as_str()` did not — one finding shipped
        /// `harness policy permissions generate --profile baseline`, a string
        /// no dispatcher recognises and no CLI accepts.
        ///
        /// Generated by `fix_commands!`, so the variant set and [`Self::ALL`]
        /// cannot disagree. `ProjectChecker::try_fix` dispatches
        /// via exhaustive `match`, so adding a variant still forces the
        /// dispatcher to handle it.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum FixCommand {
            $($variant),+
        }

        impl FixCommand {
            /// Every command, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The wire string. Exhaustive over the same declaration that built
            /// [`Self::ALL`], so the two cannot disagree.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),+
                }
            }

            /// Derived from [`Self::ALL`] — the inverse of [`Self::as_str`] by
            /// construction rather than by a second match kept in step by hand.
            pub fn from_str(s: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|c| c.as_str() == s)
            }
        }
    };
}

fix_commands! {
    CodegenSync => "harness codegen sync",
}

impl std::fmt::Display for FixCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for FixCommand {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Self::from_str(&raw).ok_or_else(|| {
            serde::de::Error::custom(format!("'{raw}' is not in the safe-fix registry"))
        })
    }
}

impl serde::Serialize for FixCommand {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl schemars::JsonSchema for FixCommand {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FixCommand".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "Command from the safe-fix registry; the only values `--fix` dispatches.",
            "enum": Self::ALL.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
        })
    }
}

/// Single finding produced by a validator / verifier / classifier.
///
/// Designed for AI consumption: `slug` is grep-able to the rule, `hint`
/// is one-line remediation, `fix_command` (if `auto_fixable`) is the
/// exact shell invocation a downstream agent can run.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Finding {
    pub slug: String,
    pub severity: Severity,
    pub location: Location,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default)]
    pub auto_fixable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_command: Option<FixCommand>,
}

/// A rule that loaded but did not fire on this input, with the reason.
/// Absence of a slug from `findings` means the rule passed; absence
/// from BOTH `findings` and `skipped_rules` means the rule never ran.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SkippedRule {
    pub slug: String,
    pub reason: String,
}

/// List-shaped response payload.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListResponse<T: schemars::JsonSchema> {
    pub items: Vec<T>,
    pub total: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_rules: Vec<SkippedRule>,
}

impl<T: schemars::JsonSchema> ListResponse<T> {
    pub fn new(items: Vec<T>) -> Self {
        let total = items.len();
        Self {
            items,
            total,
            skipped_rules: Vec::new(),
        }
    }
    pub fn with_skipped(mut self, skipped: Vec<SkippedRule>) -> Self {
        self.skipped_rules = skipped;
        self
    }
}

/// `warnings` is always serialized, empty included: Article II states the
/// success shape as `{ok, data?, error?, warnings[]}`, and a consumer reading
/// `envelope.warnings.length` should not have to know that the quiet case
/// spells the empty list as a missing key.
#[derive(Serialize)]
struct SuccessEnvelope<'a, T: Serialize> {
    ok: bool,
    data: T,
    warnings: &'a [Warning],
}

#[derive(Serialize)]
struct ErrorDetail<'a> {
    code: &'a str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<&'a Location>,
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    ok: bool,
    error: ErrorDetail<'a>,
}

/// Write a success envelope (one JSON object + newline).
pub fn write_success<T: Serialize, W: Write>(
    out: &mut W,
    data: T,
    warnings: &[Warning],
) -> io::Result<()> {
    let env = SuccessEnvelope {
        ok: true,
        data,
        warnings,
    };
    serde_json::to_writer(&mut *out, &env)?;
    out.write_all(b"\n")?;
    Ok(())
}

/// Owned, schema-derivable representation of the envelope shape.
/// Used by `harness export schema envelope` to describe the contract.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EnvelopeShape {
    /// `true` for success, `false` for error.
    pub ok: bool,
    /// Payload for success envelopes. Shape depends on the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Present for error envelopes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorPayload>,
    /// Non-blocking warnings attached to success envelopes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
}

/// Write an error envelope (one JSON object + newline).
pub fn write_error<W: Write>(out: &mut W, error: &Error) -> io::Result<()> {
    let body = ErrorDetail {
        code: error.code().as_str(),
        message: error.to_string(),
        hint: error.hint(),
        location: error.location(),
    };
    let env = ErrorEnvelope {
        ok: false,
        error: body,
    };
    serde_json::to_writer(&mut *out, &env)?;
    out.write_all(b"\n")?;
    Ok(())
}
