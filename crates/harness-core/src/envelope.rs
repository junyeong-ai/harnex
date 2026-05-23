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
        Self { path: path.into(), line: None, col: None }
    }
    pub fn line(path: impl Into<PathBuf>, line: u32) -> Self {
        Self { path: path.into(), line: Some(line), col: None }
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
/// `Blocker` is the highest tier — non-zero exit, prevents commits / CI
/// passage. New tiers may be added at the top when a class of findings
/// needs to outrank Blocker (e.g., security violations); doing so
/// requires updating `Severity::rank` and every `has_blocker` pattern
/// (the compiler enforces both via exhaustive `match`).
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
    pub fix_command: Option<String>,
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
        Self { items, total, skipped_rules: Vec::new() }
    }
    pub fn with_skipped(mut self, skipped: Vec<SkippedRule>) -> Self {
        self.skipped_rules = skipped;
        self
    }
}

fn slice_is_empty<T>(s: &&[T]) -> bool {
    s.is_empty()
}

#[derive(Serialize)]
struct SuccessEnvelope<'a, T: Serialize> {
    ok: bool,
    data: T,
    #[serde(skip_serializing_if = "slice_is_empty")]
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
    let env = SuccessEnvelope { ok: true, data, warnings };
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
    let env = ErrorEnvelope { ok: false, error: body };
    serde_json::to_writer(&mut *out, &env)?;
    out.write_all(b"\n")?;
    Ok(())
}
