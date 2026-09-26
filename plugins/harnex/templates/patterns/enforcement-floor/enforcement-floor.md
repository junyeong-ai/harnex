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

`harnex guard floor` blocks exactly two things, and they are wired
separately because their costs are not alike. The tripwire — a git command
that would skip the hook stack (`--no-verify`, `commit -n`, a
`core.hooksPath` reroute, compound commands included) — reads no
configuration and freezes nothing, so `hooks/check-floor.sh` is wired for
`Bash` in every scaffold and keeps standing where `harness.toml` has been
removed, which no Edit can do but any Bash call can. This pattern adds the
second entry, `Edit|Write|MultiEdit`, which freezes the files that define what
the gates verify. A failing gate is fixed at its cause, never by weakening what
the gate verifies.

What the tripwire reads is the command line, as the shell composes it. So it
answers about a git invocation spelled there, and a command that reaches git
through another program — `mise exec -- git …`, `npx … git …`, a shell alias,
`sh -c` — is not one. Those are the permission surface's, and a session running
without permission checks has neither. Wire this pattern for what it is: the
floor under a command typed directly, not a boundary around the capability.

The freeze is the half with a price: it covers `harness.toml` and
`.claude/settings.json`, so in a repository where the harness is the work
product it fires on most commits, and a grant left standing to answer that
prints its notice so often it stops being a signal. Install this pattern
where the gate files are not the work product.

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
- **A block is a message, not a wall.** Fix the failing gate at its cause. A
  bypass the operator truly needs is theirs to run, outside the agent, and the
  block names no way around itself: the loop the gate bounds is what reads it
  first, and a message that hands it an exit is a floor that teaches how to
  leave. Each hatch is named in the guardrail's own source, where the operator
  looks for it, and says so on the commit it skips.
