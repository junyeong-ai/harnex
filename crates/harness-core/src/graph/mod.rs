//! # graph — read-only bridge to a `nodex` document graph
//!
//! Wraps the `nodex` CLI behind a [`NodexRunner`] trait so the toolkit can
//! query backlinks / orphans / stale / nodes-of-kind without re-deriving
//! the graph at every harness invocation. The default [`DefaultNodexRunner`]
//! spawns the real `nodex` binary; tests inject a mock runner.
//!
//! ## What this module refuses to do
//!
//! - Never mutate the graph. All operations are read-only queries.
//! - Never embed nodex's schema as Rust types — `NodeRef` flattens unknown
//!   fields into `extra`. Upstream schema changes don't break this bridge.
//! - Never silently fall back when `nodex` is missing — `detect` returns
//!   `None` and the caller decides how to degrade.

pub mod client;
pub mod node;

pub use client::{DefaultNodexRunner, NodexClient, NodexRunner};
pub use node::{GraphDiff, NodeRef};
