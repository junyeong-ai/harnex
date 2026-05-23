//! # guard — Claude Code runtime adapter
//!
//! Three surfaces:
//! - [`HookEvent`] — typed parser for hook stdin JSON (29 documented events
//!   per <https://code.claude.com/docs/en/hooks>).
//! - [`HookRunner`] — replacement for fragile `_runner.sh` / `_stop_runner.sh`
//!   patterns. [`HookRunner::run`] propagates the inner exit code (for
//!   PreToolUse/PostToolUse). [`HookRunner::run_stop`] suppresses non-zero
//!   inner exits to 0 (for Stop/SubagentStop) — observed code captured in
//!   the envelope, never propagated to git, preventing Stop-loop traps.
//!   Both fail-open when the project root cannot be resolved.
//! - [`StopAuditor`] — handles the Stop event; spawns fresh-context critique
//!   skill when changes exist; bounded retry counter prevents
//!   premature-termination defect classes.
//!
//! ## What this module refuses to do
//!
//! - Never block the Stop event silently — failures escalate via Block
//!   decision with reason.
//! - Never spawn arbitrary commands. The critique skill name is config-pinned.
//! - Never bypass the retry counter — multi-retry loops without bound are
//!   the root cause of premature-termination defects; bounded retries are
//!   the cure.

pub mod hook_event;
pub mod hook_runner;
pub mod stop_audit;

pub use hook_event::HookEvent;
pub use hook_runner::{HookRunOutcome, HookRunner};
pub use stop_audit::{StopAuditor, StopDecision};
