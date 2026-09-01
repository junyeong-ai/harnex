---
paths:
  - "harness.toml"
  - ".claude/settings.json"
governs:
  concept: the enforcement-surface freeze and the hook-bypass tripwire
  live_truth:
    - harness.toml
    - .claude/settings.json
---

# Enforcement floor — the gates cannot be edited past

`harnex guard floor` runs on PreToolUse for `Bash` and `Edit|Write|MultiEdit`
(two entries in `.claude/settings.json`, wired directly, not through
`_runner.sh`). It blocks exactly two things: a git command that would skip
the hook stack (`--no-verify`, `commit -n`, a `core.hooksPath` reroute —
compound commands included), and a write to a file that defines what the
gates verify. A failing gate is fixed at its cause, never by weakening what
the gate verifies.

## The contract

- **The protected set has one owner.** `harness.toml` `[guard.floor]`
  `protected_paths` names the project's gate-defining files; `harness.toml`,
  `.claude/settings.json` and `.claude/settings.local.json` are built into
  the floor itself. Do not restate the list here or anywhere else.
- **Break-glass is the operator's, read live.** Deliberate harness work
  proceeds when the operator sets `HARNEX_ALLOW_FLOOR_EDIT: "1"` in the
  `env` block of the **main** checkout's `.claude/settings.local.json` —
  effective on the next check, revoked the moment the entry is removed. A
  granted edit still surfaces a `[floor-edit allowed …]` notice: the one
  signal the freeze was bypassed.
- **The two halves fail in opposite directions, deliberately.** A check that
  cannot evaluate allows with a visible `[floor-check skipped: …]` notice
  (not proven guilty); an override that cannot be read is an absent one (not
  proven authorised). Do not "fix" either direction into the other.
- **Tripwire, not boundary.** A shell is Turing-complete: a write smuggled
  through Bash (`sed -i`, redirection, heredoc) and an obfuscated bypass (a
  git alias, `sh -c`, env-var config injection) are out of scope. The
  authoritative backstop is the server-side CI re-run of the same gates —
  keep it green and un-bypassed.
- **A block is a message, not a wall.** Fix the failing gate at its cause; a
  harnex-generated hook names its own escape hatch, which skips that one
  check and never the stack. A bypass the operator truly needs is run by the
  operator, outside the agent.
