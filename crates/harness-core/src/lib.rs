//! # harness-core
//!
//! Embeddable library powering the `harnex` CLI. All deterministic logic
//! for harness engineering on Claude Code projects lives here. The CLI is
//! a thin clap wrapper that emits the JSON envelope.
//!
//! ## What this crate refuses to do
//!
//! - No async, no AI dependencies, and no network at command time beyond
//!   [`ask::serve`]'s single-answer listener on 127.0.0.1.
//! - No project domain vocabulary in source — every project-specific shape
//!   derives from `harness.toml`.
//! - No string-matched errors — every failure surfaces as a typed
//!   [`error::Error`] with a stable [`error::ErrorCode`].

pub mod always_loaded;
pub mod ask;
pub mod audit;
pub mod check;
pub mod codegen;
pub mod config;
pub mod context;
pub mod envelope;
pub mod error;
pub mod evidence;
pub mod export;
mod git;
pub mod glob_root;
pub mod governs;
pub mod graph;
pub mod guard;
pub mod lifecycle;
mod markdown;
pub mod path_guard;
pub mod plan;
pub mod policy;
pub mod routines;
pub mod scaffold;
pub mod sentinel;
pub mod session;
pub mod spec;
pub mod telemetry;
pub mod validate;
mod wire_enum;

pub use error::{Error, ErrorCode, Result};
