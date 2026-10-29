---
paths:
  - "crates/harness-core/src/guard/**"
governs:
  concept: the Claude Code hook adapter
  live_truth:
    - crates/harness-core/src/guard
---

# guard — Claude Code runtime adapter

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

FloorAuditor (`guard::floor`) handles PreToolUse for Bash and
Edit|Write|MultiEdit: the hook-bypass tripwire plus the enforcement-surface
freeze. Configuration is the freeze's precondition alone — the tripwire judges
a command line and reads none, so `FloorAuditor::undeclared(reason)` stands it
up where `harness.toml` declares no `[guard.floor]` and equally where there is
no `harness.toml` to read: that file is frozen against the Edit tools but not
against Bash, so a tripwire gated on it would be one `rm` away. The two are
wired as separate PreToolUse entries because their costs differ: the freeze
sends every edit to `harness.toml` and the settings files through the
operator's grant, which a repository editing its own harness pays on ordinary
work, while the tripwire refuses only a command that skips the hook stack. A
Bash write into the floor is
not read from the command line, because what it writes is every started
program's to decide: `floor::sandbox` owns the floor's projection into
`sandbox.filesystem.denyWrite`, which the OS enforces on each of those
processes, and `check`'s `guard.floor` arm reports a frozen path it leaves
uncovered. The command-line reading stays with the hook bypass, which no OS
layer sees. The auditor's two halves fail in deliberately opposite
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
violation; unreadable stdin, an unparseable command line, or a write the
freeze has no declaration to judge exits 0 with a `[floor-check skipped: …]`
systemMessage — a broken floor must degrade to the project's other gates,
never block every tool call. A granted protected write emits the
`[floor-edit allowed …]` notice: the one signal the freeze was bypassed. The
freeze resolves the project root from `harness.toml` discovery like every
config-bearing command; a discovery failure is the reason its skip carries,
and reaches the tripwire not at all. The scaffold wires it through
`hooks/check-floor.sh` rather than directly, because a hook `command` is exec
form with no shell: an absent oracle would fail the hook on every tool call,
and one too old to carry the subcommand would exit 2 — the block code — on
every command it was asked about. The arm probes the subcommand and leaves
the call alone unless it parses. A second probe splits that silence: an
oracle absent or predating the subcommand says nothing, and one answering
neither probe is a floor down behind a name that still resolves, which the arm
reports on `systemMessage`.

A floor slow enough to reach its hook timeout is a floor that has stopped
holding: the hook is killed, the tool call proceeds, and neither the operator
nor the model is told (`plugins/harnex/reference/spec-facts.md`). What the scan
spends over a command line is therefore a correctness property, and
`command_line.rs` holds it to one pass per reading, with the readings one line
can demand bounded by `MAX_NESTING_DEPTH` — a heredoc body costs two, and what
nests inside one recurses under that same cap.

`guard::telemetry` (`harnex guard telemetry-emit`) handles PostToolUse /
PostToolUseFailure: it records one `harness_invocation` event — the invoked
element's slug and the outcome — through `session::asset_of`, the one owner
of the tool→element mapping (`Skill`→`skill`, `Task`/`Agent`→`subagent_type`),
so the recorded set cannot drift from the transcript reader's. Outcome is the
hook event's identity (`PostToolUse` = ok, `PostToolUseFailure` = failed),
never a payload field. It writes nothing on stdout and always exits 0: every
reason it cannot record — no `[telemetry]`, the Kind undeclared, a write
failure — is a silent no-op, because telemetry must never block the tool call
that triggered it. Only the element's slug (its own name, not operator
content) and the outcome cross; the closed `payload_schema` rejects the rest.
