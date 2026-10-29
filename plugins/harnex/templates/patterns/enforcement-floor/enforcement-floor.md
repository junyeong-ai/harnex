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

Every scaffold wires the tripwire: `hooks/check-floor.sh` on `Bash` refuses a
git command that would skip the hook stack (`--no-verify`, `commit -n`, a
`core.hooksPath` reroute). It reads the command line, so a git invocation
through another program's arguments — `mise exec -- git …`, `sh -c` — is the
permission surface's. This pattern adds the freeze: the `Edit|Write|MultiEdit`
entry on the files that define what the gates verify, and a
`sandbox.filesystem.denyWrite` entry for each so a Bash command is held to
them too. A failing gate is fixed at its cause, never by weakening what the
gate verifies.

## The contract

- **The protected set has one owner.** `harness.toml` `[guard.floor]`
  `protected_paths` names the project's gate-defining files; `harness.toml`,
  `.claude/settings.json` and `.claude/settings.local.json` are built into
  the floor itself. Do not restate the list; `harnex check` names each
  `denyWrite` entry it lacks. What the sandbox refuses on its own — the
  settings files among it — needs none.
- **Bash writes are the sandbox's.** The sandbox enforces `denyWrite` on every
  process a command starts, and only in a session it runs in. The grant opens
  the Edit tools only, so a Bash write into the floor — and a checkout or
  merge that must rewrite a frozen file — is the operator's to run, outside the
  agent.
- **Break-glass is the operator's, read live.** Deliberate harness work
  proceeds when the operator sets `HARNEX_ALLOW_FLOOR_EDIT: "1"` in the `env`
  block of the **main** checkout's `.claude/settings.local.json`, and stops the
  moment the entry is removed. A granted edit prints a `[floor-edit allowed …]`
  notice: the one signal the freeze was bypassed.
- **A block is a message, not a wall.** Fix the failing gate at its cause. A
  bypass the operator truly needs is theirs to run, outside the agent; the
  block names no way around itself, because the loop the gate bounds reads it
  first. The server-side CI re-run of the same gates is the backstop — keep it
  green and un-bypassed.
