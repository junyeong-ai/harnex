//! # harness-core
//!
//! Embeddable library powering the `harnex` CLI. All deterministic logic
//! for harness engineering on Claude Code projects lives here. The CLI is
//! a thin clap wrapper that emits the JSON envelope.
//!
//! ## Modules
//!
//! - [`config`] — loads + validates `harness.toml`. SSoT for project-specific shape.
//! - [`envelope`] — JSON envelope contract every command emits.
//! - [`error`] — typed Error enum with stable ErrorCode strings.
//! - [`path_guard`] — safe write primitives (`write_atomic` + `append_line`).
//! - [`glob_root`] — a glob pattern rooted at a literal directory.
//! - [`evidence`] — provenance verifier with pluggable strategies.
//! - [`telemetry`] — append-only closed-schema event ledger.
//! - [`codegen`] — cross-file sentinel-block sync.
//! - [`policy`] — permission profiles + version pins.
//! - [`scaffold`] — the composition manifest a generated harness is built from.
//! - [`spec`] — when each closed vocabulary was last measured against the docs.
//! - [`validate`] — rule / skill / settings / commit-msg checks.
//! - [`audit`] — harness-engineering compliance gate (spec drift,
//!   managed-region integrity).
//! - [`lifecycle`] — observation aggregation + retirement classification.
//! - [`guard`] — Claude Code runtime adapter (hook events / runners / Stop audit).
//! - [`plan`] — the spec-workflow review grammar (finding rows, dispositions,
//!   decision-log convergence), held by `plan audit`.
//! - [`check`] — unified validation gate.
//! - [`export`] — JSON Schema emission.
//! - [`graph`] — read-only nodex CLI bridge.
//! - [`session`] — observed behaviour read from Claude Code transcripts.
//!
//! ## What this crate refuses to do
//!
//! - No async, no network at command time, no servers, no AI dependencies.
//! - No project domain vocabulary in source — every project-specific shape
//!   derives from `harness.toml`.
//! - No string-matched errors — every failure surfaces as a typed
//!   [`error::Error`] with a stable [`error::ErrorCode`].

pub mod audit;
pub mod check;
pub mod codegen;
pub mod config;
pub mod envelope;
pub mod error;
pub mod evidence;
pub mod export;
pub mod glob_root;
pub mod governs;
pub mod graph;
pub mod guard;
pub mod lifecycle;
pub mod path_guard;
pub mod plan;
pub mod policy;
pub mod scaffold;
pub mod sentinel;
pub mod session;
pub mod spec;
pub mod telemetry;
pub mod validate;
mod wire_enum;

pub use error::{Error, ErrorCode, Result};
