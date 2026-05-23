//! # harness-core
//!
//! Embeddable library powering the `harness` CLI. All deterministic logic
//! for harness engineering on Claude Code projects lives here. The CLI is
//! a thin clap wrapper that emits the JSON envelope.
//!
//! ## Modules
//!
//! - [`config`] — loads + validates `harness.toml`. SSoT for project-specific shape.
//! - [`envelope`] — JSON envelope contract every command emits.
//! - [`error`] — typed Error enum with stable ErrorCode strings.
//! - [`path_guard`] — safe write primitives (`write_atomic` + `append_line`).
//! - [`evidence`] — provenance verifier with pluggable strategies.
//! - [`telemetry`] — append-only closed-schema event ledger.
//! - [`codegen`] — cross-file sentinel-block sync.
//! - [`policy`] — permission profiles + version pins.
//! - [`validate`] — rule / skill / settings / commit-msg checks.
//! - [`lifecycle`] — observation aggregation + retirement classification.
//! - [`guard`] — Claude Code runtime adapter (hook events / runners / Stop audit).
//! - [`check`] — unified validation gate.
//! - [`export`] — JSON Schema emission.
//! - [`init`] — project scaffolder + hook generation.
//! - [`graph`] — read-only nodex CLI bridge.
//!
//! ## What this crate refuses to do
//!
//! - No async, no network at command time, no servers, no AI dependencies.
//! - No project domain vocabulary in source — every project-specific shape
//!   derives from `harness.toml`.
//! - No string-matched errors — every failure surfaces as a typed
//!   [`error::Error`] with a stable [`error::ErrorCode`].

pub mod check;
pub mod codegen;
pub mod config;
pub mod envelope;
pub mod error;
pub mod evidence;
pub mod export;
pub mod graph;
pub mod guard;
pub mod init;
pub mod lifecycle;
pub mod path_guard;
pub mod policy;
pub mod telemetry;
pub mod validate;

pub use error::{Error, ErrorCode, Result};
