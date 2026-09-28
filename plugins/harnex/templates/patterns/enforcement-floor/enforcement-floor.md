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
the gates verify, and holds a Bash command to the same files in the sandbox:
each frozen path gets the `sandbox.filesystem.denyWrite` entry `harnex check`
names for it. A failing gate is fixed at its cause, never by weakening what
the gate verifies.

What the tripwire reads is the command line, as the shell composes it, and a
heredoc body that line feeds. So it answers about a git invocation spelled in
either, and a command that reaches git through another program's own argument
— `mise exec -- git …`, `npx … git …`, a shell alias, `sh -c` — is not one. Those are the permission surface's, and a session running
without permission checks has neither. Wire this pattern for what it is: the
floor under a command typed directly, not a boundary around the capability.

The freeze is the half with a price: every edit to `harness.toml` or
`.claude/settings.json` goes through the operator's break-glass grant. Where
editing those files is the repository's ordinary work, the grant is switched
on for each such change or left standing, and a notice printed on every such
edit stops being a signal. Install this pattern where editing the gate files
is not ordinary work.

## The contract

- **The protected set has one owner.** `harness.toml` `[guard.floor]`
  `protected_paths` names the project's gate-defining files; `harness.toml`,
  `.claude/settings.json` and `.claude/settings.local.json` are built into
  the floor itself. Do not restate the list here or anywhere else;
  `sandbox.filesystem.denyWrite` is its projection, and `harnex check` names
  each entry it lacks. What the sandbox refuses on its own — the settings
  files, `.mcp.json`, and `.claude`'s `hooks`, `skills`, `commands` and
  `agents` among it — needs none.
- **Bash writes are the sandbox's.** What a shell command writes is decided by
  every program it starts, so no reading of the command line refuses it; the
  sandbox enforces `denyWrite` on each of those processes. It holds only where
  the sandbox runs, and does nothing where it is off. The two halves cover each
  other's tool: a session the sandbox refuses can reach for Write, and one the
  freeze refuses can reach for Bash. A session that skips permission prompts
  can retry a denied command unsandboxed without being asked unless
  `sandbox.allowUnsandboxedCommands` is `false`. The grant opens the
  Edit tools only, so a Bash write into the floor — and a git checkout or merge
  that must rewrite a frozen file — is the operator's to run, outside the
  agent.
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
- **Tripwire, not boundary.** A shell is Turing-complete, so these stay out
  of scope: an obfuscated bypass (a git alias, `sh -c`), a reroute that hands git a
  file instead of naming the key (`include.path`, `GIT_CONFIG_GLOBAL`), and
  a backtick substitution on the command line, whose mark is also the
  code-span mark — reading it there refuses documents that quote a flag,
  while in a heredoc body the shell expands it is the shell that pairs the
  backticks and runs what they hold, so there the body is read. Config
  carried in through the
  environment, and a `$(…)` the line runs git inside, are read rather than
  excused. The authoritative backstop is the server-side CI re-run of the
  same gates — keep it green and un-bypassed.
- **Writing about the bypass is not running it.** The tripwire reads what the
  shell expands, so a mention is text the shell leaves alone: single quotes on
  the command line, and a heredoc body whose delimiter carries any quoting
  (`<<'EOF'`, `<<"EOF"`, `<<\EOF`), where nothing expands at all. A commit
  message or a document written that way may name the flag freely. Under a
  bare delimiter the body expands, and only `\`, `$` and a backtick act
  there — a quote or a `#` around a `$(…)` or a backtick pair hides neither
  from the shell nor from the check. A line whose **first** word is the bypass
  is refused inside any heredoc, because the body may be a script rather than
  a document.
- **A block is a message, not a wall.** Fix the failing gate at its cause. A
  bypass the operator truly needs is theirs to run, outside the agent, and the
  block names no way around itself: the loop the gate bounds is what reads it
  first, and a message that hands it an exit is a floor that teaches how to
  leave. Each hatch is named in the guardrail's own source, where the operator
  looks for it, and says so on the commit it skips.
