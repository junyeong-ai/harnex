//! # guard — Claude Code runtime adapter
//!
//! Three surfaces:
//! - [`HookEvent`] — typed parser for hook stdin JSON (event surface per
//!   <https://code.claude.com/docs/en/hooks>).
//! - [`project_dir`] — the `${CLAUDE_PROJECT_DIR}` anchor grammar, one
//!   function per field kind: a shell-interpreted `command` and a literal
//!   `args` element are not parsed alike.
//! - [`StopAuditor`] — handles the Stop event; spawns fresh-context critique
//!   skill when changes exist; bounded retry counter prevents
//!   premature-termination defect classes.
//! - [`FloorAuditor`] — handles PreToolUse for Bash and the Edit tools; the
//!   enforcement-surface freeze and the hook-bypass tripwire ([`floor`]).
//! - [`telemetry`] — handles PostToolUse / PostToolUseFailure; records one
//!   harness-element invocation per call through the shared `asset_of` mapping,
//!   silent and never blocking.
//!
//! ## What this module refuses to do
//!
//! - Never block the Stop event on an unavailable audit. Report a Skip or
//!   Escalate to the operator; only a recognised gating finding requests continuation.
//! - The Stop auditor spawns only its configured changes probe and critique.
//! - Never bypass the retry counter — multi-retry loops without bound are
//!   the root cause of premature-termination defects; bounded retries are
//!   the cure.

/// The two keys a guard writes when it allows the event and still has
/// something to say. `systemMessage` reaches the operator; `suppressOutput`
/// keeps the same line out of the transcript the model reads.
///
/// Both `guard floor` and `guard stop-audit` speak this pair instead of the
/// envelope, which `.claude/rules/envelope.md` records as its exception and
/// `.claude/rules/guard.md` explains. The shipped hook arms spell the pair a
/// second time, because what they report is a binary that never ran; a
/// rename here has to reach them or they report to no one.
/// `guard_channel_prose_sync` holds every one of those to these names.
pub const OPERATOR_CHANNEL_KEY: &str = "systemMessage";
pub const SUPPRESS_OUTPUT_KEY: &str = "suppressOutput";

pub mod floor;
pub mod hook_event;
pub mod project_dir;
pub mod stop_audit;
pub mod telemetry;

pub use floor::{FloorAuditor, FloorDecision};
pub use hook_event::HookEvent;
pub use project_dir::{path_in_argument, paths_in_command};
pub use stop_audit::{StopAuditor, StopDecision};
pub use telemetry::{EmitOutcome, HARNESS_INVOCATION_KIND, OUTCOME_FIELD, SURFACE_FIELD};
