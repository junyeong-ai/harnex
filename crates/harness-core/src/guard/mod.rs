//! # guard — Claude Code runtime adapter
//!
//! Three surfaces:
//! - [`HookEvent`] — typed parser for hook stdin JSON (event surface per
//!   <https://code.claude.com/docs/en/hooks>).
//! - [`HookRunner`] — replacement for fragile `_runner.sh` / `_stop_runner.sh`
//!   patterns. [`HookRunner::run`] propagates the inner exit code (for
//!   PreToolUse/PostToolUse). [`HookRunner::run_stop`] suppresses non-zero
//!   inner exits to 0 (for Stop/SubagentStop) — observed code captured in
//!   the envelope, never propagated to git, preventing Stop-loop traps.
//!   Both fail-open when the project root cannot be resolved.
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
//! - Never block the Stop event silently — failures escalate via Block
//!   decision with reason.
//! - Never spawn arbitrary commands. The critique skill name is config-pinned.
//! - Never bypass the retry counter — multi-retry loops without bound are
//!   the root cause of premature-termination defects; bounded retries are
//!   the cure.

/// The two keys a guard writes when it allows the event and still has
/// something to say. `systemMessage` reaches the operator; `suppressOutput`
/// keeps the same line out of the transcript the model reads.
///
/// Both `guard floor` and `guard stop-audit` speak this pair instead of the
/// envelope, which `.claude/rules/envelope.md` records as its exception and
/// `.claude/rules/guard.md` explains. `guard_channel_prose_sync` holds those
/// two documents to these names.
pub const OPERATOR_CHANNEL_KEY: &str = "systemMessage";
pub const SUPPRESS_OUTPUT_KEY: &str = "suppressOutput";

pub mod floor;
pub mod hook_event;
pub mod hook_runner;
pub mod project_dir;
pub mod stop_audit;
pub mod telemetry;

pub use floor::{FloorAuditor, FloorDecision};
pub use hook_event::HookEvent;
pub use hook_runner::{HookRunOutcome, HookRunner};
pub use project_dir::{path_in_argument, paths_in_command};
pub use stop_audit::{StopAuditor, StopDecision};
pub use telemetry::{EmitOutcome, HARNESS_INVOCATION_KIND, OUTCOME_FIELD, SURFACE_FIELD};
