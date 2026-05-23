---
paths:
  - "crates/harness-core/src/guard/**"
---

# guard — Claude Code runtime adapter

HookEvent parses stdin JSON for Claude Code hook events. The toolkit
does not model event-specific fields as typed Rust structs — the
29-event surface evolves upstream. Common fields are extracted; the
rest is accessible via `HookEvent::field(key)`.

HookRunner replaces fragile `_runner.sh` / `_stop_runner.sh` patterns.
Resolves project root via `git rev-parse --show-toplevel`. If unresolved,
returns `SkippedFailOpen` and emits `[harness-skipped: …]` to stderr —
never penalizes the user for env drift.

Two variants:
- [`HookRunner::run`] (`harness guard hook-run`) — propagates the inner
  exit code. Used for PreToolUse / PostToolUse / UserPromptSubmit / etc.
  where a non-zero exit blocks the agent action.
- [`HookRunner::run_stop`] (`harness guard hook-stop`) — observes the
  inner exit code but ALWAYS returns 0 to git, capturing the observed
  code in the envelope. Used for Stop / SubagentStop where a non-zero
  exit would trap the agent in a Stop loop (per Claude Code spec, Stop
  hook non-zero exits trigger re-stop). Non-zero observations emit a
  `[harness-stop-advisory]` line to stderr for operator visibility.

StopAuditor handles the Stop event in three phases:
1. `has_changes_check` — exit 0 means no changes, allow stop.
2. Bump per-session retry counter via `path_guard::write_atomic`.
   Exceeding `max_retries` escalates with a Block reason.
3. Spawn the configured critique skill via `claude --print`. Parse the
   returned JSON envelope; any finding with severity in
   {blocker} blocks the stop.

The retry counter is the deterministic antidote to single-loop
self-grade inflation. Never bypass — bounded retries are the cure.
