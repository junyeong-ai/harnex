//! # guard — Claude Code runtime adapter
//!
//! - [`project_dir`] — the `${CLAUDE_PROJECT_DIR}` anchor grammar, one
//!   function per field kind: a shell-interpreted `command` and a literal
//!   `args` element are not parsed alike.
//! - [`FloorAuditor`] — handles PreToolUse for Bash and the Edit tools; the
//!   enforcement-surface freeze and the hook-bypass tripwire ([`floor`]).
//! - [`telemetry`] — handles PostToolUse / PostToolUseFailure; records one
//!   harness-element invocation per call through the shared `asset_of` mapping,
//!   silent and never blocking.
//!
//! ## What this module refuses to do
//!
//! - Never block an event on its own inability to evaluate it. A guard that
//!   cannot judge allows and says why on the operator's channel; only a
//!   detected floor violation blocks.

/// The two keys a guard writes when it allows the event and still has
/// something to say. `systemMessage` reaches the operator; `suppressOutput`
/// keeps the same line out of the transcript the model reads.
///
/// `guard floor` speaks this pair instead of the envelope, and so does a
/// `guard` invocation that does not parse; `.claude/rules/envelope.md` records
/// both as exceptions. The shipped hook arms spell the pair a
/// second time, because what they report is a binary that never ran; a
/// rename here has to reach them or they report to no one.
/// `guard_channel_prose_sync` holds every one of those to these names.
pub const OPERATOR_CHANNEL_KEY: &str = "systemMessage";
pub const SUPPRESS_OUTPUT_KEY: &str = "suppressOutput";

pub mod floor;
pub mod project_dir;
pub mod telemetry;

pub use floor::{FloorAuditor, FloorDecision};
pub use project_dir::{path_in_argument, paths_in_command};
pub use telemetry::{EmitOutcome, HARNESS_INVOCATION_KIND, OUTCOME_FIELD, SURFACE_FIELD};
