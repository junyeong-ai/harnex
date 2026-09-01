---
paths:
  - "crates/harness-core/src/guard/**"
governs:
  concept: the Claude Code hook adapter and Stop auditor
  live_truth:
    - crates/harness-core/src/guard
---

# guard — Claude Code runtime adapter

HookEvent parses stdin JSON for Claude Code hook events. The toolkit
does not model event-specific fields as typed Rust structs — the
hook-event surface evolves upstream. Common fields are extracted; the
rest is accessible via `HookEvent::field(key)`.

HookRunner replaces fragile `_runner.sh` / `_stop_runner.sh` patterns.
Resolves project root via `git rev-parse --show-toplevel`. If unresolved,
returns `SkippedFailOpen` and emits `[harness-skipped: …]` to stderr —
never penalizes the user for env drift.

Variants:
- [`HookRunner::run`] (`harnex guard hook-run`) — propagates the inner
  exit code. Used for PreToolUse / PostToolUse / UserPromptSubmit / etc.
  where a non-zero exit blocks the agent action.
- [`HookRunner::run_stop`] (`harnex guard hook-stop`) — observes the
  inner exit code but ALWAYS returns 0 to git, capturing the observed
  code in the envelope. Used for Stop / SubagentStop where a non-zero
  exit would trap the agent in a Stop loop (per Claude Code spec, Stop
  hook non-zero exits trigger re-stop). Non-zero observations emit a
  `[harness-stop-advisory]` line to stderr for operator visibility.

Each is discovery over a root-taking form — `run_at` / `run_stop_at` — and the
exit-code contract lives there, as a working directory is a parameter
everywhere else in this crate. Fused, that contract was only reachable from
inside a git working tree, so it failed for anyone building from a source
release while the product behaved correctly. The fail-open branch belongs to
discovery alone: given a root there is nothing to fail open about, so
`SkippedFailOpen` is unreachable from the root-taking forms by construction.

`project_dir` owns the `${CLAUDE_PROJECT_DIR}` grammar — the one token form in
a hook that denotes a repository path by construction. Both anchor spellings
are read. A glob or a token carrying a second variable is not a path and is
skipped.

**Two functions, because a handler carries two kinds of string.**
`paths_in_command` reads a shell-interpreted `command`: a token ends at an
unescaped metacharacter or at its quote, so trailing flags are not part of the
filename. `path_in_argument` reads one literal `args` element, which nothing
splits, so a space belongs to the name and a prefix before the anchor does not.
The caller picks by the field it read — inferring the grammar from where the
anchor sits inside the string reads
`${CLAUDE_PROJECT_DIR}/hooks/run.sh --verbose` as a filename ending in
`--verbose`, and disagrees with the same command written `bash ...`.

One home because two would drift: the hook-wiring auditor and the
scaffold-manifest test ask the same question of the same grammar.

StopAuditor handles the Stop event in three phases:
1. `has_changes_check` — exit 0 means no changes, allow stop.
2. Bump per-session retry counter via `path_guard::write_atomic`.
   Exceeding `max_retries` escalates with a Block reason.
3. Spawn the configured critique skill via `claude --print`. Parse the
   returned JSON envelope; any finding that fails the gate
   (`Severity::fails_gate` — blocker or major) blocks the stop. Malformed
   critique output fails OPEN (allow stop) — Article V, the bounded retry
   counter is the loop's safety net, not a fail-closed gate.

The retry counter is the deterministic antidote to single-loop
self-grade inflation. Never bypass — bounded retries are the cure.

`harnex guard stop-audit` maps a `StopDecision::Block` to exit 2 — the
sole sanctioned exception to Article II (where exit 2 = runtime failure).
Per the Claude Code Stop-hook contract, exit 2 prevents the stop and
forces continuation; exit 1 would be non-blocking. This is intentional;
do not "normalize" it to exit 1. The envelope is still emitted on stdout.

FloorAuditor (`guard::floor`, gated on `[guard.floor]`) handles PreToolUse
for Bash and Edit|Write|MultiEdit: the enforcement-surface freeze plus the
hook-bypass tripwire. Its two halves fail in deliberately opposite
directions — violation checks fail open (inability to evaluate is a
`Skip` with a reason, never a block), while the operator's break-glass
grant fails closed (an unreadable override is an absent one). The grant is
`HARNEX_ALLOW_FLOOR_EDIT: "1"` in the **main** checkout's
`.claude/settings.local.json` env block, read live from the file — never
from the process environment, which Claude Code hot-reloads from the same
file and which is therefore a copy, not a second witness.
`canonical_repo_root` follows a linked worktree's `.git` → `commondir` to
the main checkout so a worktree cannot mint its own grant.

`harnex guard floor` speaks the PreToolUse hook contract, not the
envelope: exit 2 (reason on stderr) is reserved for a *detected*
violation; a config failure, missing `[guard.floor]`, unreadable stdin, or
unparseable command line exits 0 with a `[floor-check skipped: …]`
systemMessage — a broken floor must degrade to the project's other gates,
never block every tool call. A granted protected write emits the
`[floor-edit allowed …]` notice: the one signal the freeze was bypassed.
Wire it directly as PreToolUse (not through `_runner.sh`); it resolves the
project root from `harness.toml` discovery like every config-bearing
command.
