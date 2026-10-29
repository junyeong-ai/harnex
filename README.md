# harnex

Harness engineering for Claude Code projects. harnex has two surfaces:

- **The harnex plugin** (primary) — a Claude Code skill that *generates*
  project-fit, project-native harness tooling (hooks, `settings.json`,
  `CLAUDE.md`, path-scoped rules) into a target repo, in that repo's own
  language, from verified spec-correct templates. The value is the
  knowledge of getting the Claude Code spec right, distributed as a skill —
  not a runtime you depend on.
- **The `harnex` binary** (oracle) — a Pure-Rust, JSON-first CLI that
  deterministically verifies a harness — provenance, closed-schema telemetry,
  lifecycle, runtime guards, a unified validation gate — and puts decisions
  to the person on a local page. It is the spec-correct reference the
  plugin's templates are checked against.

## Why

Modern Claude follows in-context conventions well. What it cannot do alone
is keep its harness spec-correct as the upstream surface evolves, enforce
what the runtime would silently corrupt, or fit one harness to many
languages and module shapes. harnex centralizes the *correctness knowledge*
and emits a harness each project owns — never a shared binary every project
must couple to.

## The plugin

A single-skill plugin under `plugins/harnex/`, distributed by the marketplace
at `.claude-plugin/marketplace.json`. Install, then drive it by mode:

```
/plugin marketplace add junyeong-ai/harnex
/plugin install harnex@harnex

/harnex scaffold      # greenfield: compose a full harness from templates
/harnex extend        # brownfield: add one guardrail in the incumbent idiom
/harnex retire        # evidence for removing one, with the limit stated
/harnex audit         # read-only: gap report (drift, over-constraint, prose-only musts)
/harnex regenerate    # re-derive against the current Claude Code spec

/harnex:measure       # read your own transcripts: what you delegated, what leaks,
                      # whether the harness earns its place
```

`measure` is a command rather than a skill mode, and `session-judge` is the
sub-agent it dispatches to read instruction text. Both need the oracle; without
it they say so and stop rather than estimating from the logs. What a window
finds recurring is written to the lifecycle observation ledger, so the
promotion loop counts it across windows and `/harnex extend` is one decision
away once it crosses the thresholds.

It also comes last. `measure` answers whether a harness earns its place and
what the operator repeats every session, so it has something to say once there
is a harness and a few weeks of transcripts — `scaffold` is where a project
without one starts.

It detects the stack from lockfile + manifest (TypeScript/Node, Python/uv,
Rust/cargo, JVM/Gradle-Maven for Java and Kotlin) and composes the harness
from `templates/scaffold.toml`, which declares every artifact in two tiers: a
**foundation** tier with no language dependency, and a **language** tier that
needs a detected profile. The manifest is the only list of what a harness
contains — read it rather than a summary. A stack harnex has no profile for
still receives the foundation tier and a report of what is missing, and a repo
holding two stacks receives the language tier once per stack. It never
free-generates a hook or permission rule. Knowledge lives in `reference/`,
which the skill reads on demand.

## The oracle binary

```bash
curl -fsSL https://github.com/junyeong-ai/harnex/raw/main/scripts/install.sh | bash
```

Takes the binary this project releases for your platform, verifies its sha256,
and installs it to `~/.local/bin` — no Rust toolchain involved. Linux archives
link musl statically, so one per architecture runs on any distribution.

macOS and Linux, on x86-64 and arm64, have a release binary. Anywhere else the
installer says so and builds that release's tag from source instead.

```bash
scripts/install.sh --version v1.2.3   # a specific release rather than the latest
scripts/install.sh --build            # build from source instead of downloading
scripts/install.sh --check            # what is installed, changing nothing
scripts/install.sh --help             # every option
```

`--build` runs cargo in the clone, so rustup uses the Rust its
`rust-toolchain.toml` pins, the compiler CI builds with; for the clone's own
tree the script checks for it against the `rust-version` in `Cargo.toml` rather
than holding a second copy. A tag or commit — `--rev`, or a release built where
no binary fits — is fetched by cargo, which holds the compiler to that tree's
own `rust-version`.
`cargo build --release` still works for a local build without installing.

Every asset is built by `.github/workflows/release.yml` from the tag it is
attached to, and carries provenance saying so:

```bash
gh attestation verify harnex-<target>.tar.gz --repo junyeong-ai/harnex
```

Installing the plugin does not install the binary. Enabling a plugin is not
consent to put an executable on the machine, so the two are separate acts and
the plugin reports the oracle as missing rather than fetching it.

### Pin the oracle per repository

`harness.toml` pins the release a harness was written from, up to the next
minor — `[meta] harnex_version = ">=0.37.1, <0.38"` — and a binary outside that
range refuses every command that loads it, except the `guard` hooks, which
skip rather than block a session. A break ships only in a new minor; a patch
changes a gate's verdict on an unchanged tree only to catch a defect that was
already there or to withdraw a regression it shipped. So the pin moves at a
breaking release, or when a harness starts using what a later patch added.

Install the binary directly, at a release the pin admits —
`curl -fsSL https://github.com/junyeong-ai/harnex/raw/main/scripts/install.sh | bash -s -- --version <tag>`,
or `scripts/install.sh --version <tag>` from a clone — rather than through a
tool manager's shim. The hooks call harnex on every commit and session event,
a shim adds its own resolution to each call, and once installed it answers
`harnex` for every repository whose `PATH` puts the shims first. A machine therefore holds one
binary, and repositories pinned to different minors move to one, each by its
own decision; until then the one left behind refuses with
`CONFIG_VERSION_MISMATCH`.

## IDE integration

`schemas/harness.schema.json` ships in this repo. Point your TOML
language server at it for autocomplete + validation on `harness.toml`:

- **Taplo / VS Code Even-Better-TOML**: add a `#:schema` directive at the
  top of `harness.toml` naming the schema of the release your pin admits —
  `https://raw.githubusercontent.com/junyeong-ai/harnex/<tag>/schemas/harness.schema.json`
  — or a `file://` URL of `schemas/harness.schema.json` in a local checkout.
- **IntelliJ family**: Languages & Frameworks → Schemas and DTDs → JSON
  Schema Mappings → add `harness.schema.json` for the pattern `harness.toml`.

Regenerate after upstream schema changes:

```bash
harnex export schema config --raw > schemas/harness.schema.json
```

(`--raw` emits the bare schema; without it the schema is wrapped in the
standard JSON envelope for programmatic consumers.)

## Oracle quickstart

Scaffolding a fresh harness is the plugin's job (`/harnex scaffold`). The
binary verifies one once it exists:

```bash
cd your-project/

# Start from an example config (or let /harnex scaffold generate it)
cp <harnex>/examples/harness.toml.minimal harness.toml

# Unified gate — every enabled validator in one JSON envelope
harnex check
```

`examples/harness.toml.minimal` enables just evidence (provenance verifier)
and telemetry (event ledger) — the smallest useful surface.
`examples/harness.toml.team` is a broad config (adds
validate.rules/skills, policy.permissions, lifecycle, …). Start
from one and extend with `[[kinds]]`, `[[lifecycle.consumer_detectors]]`,
`[[policy.versions]]`, `[validate.commit_msg]` as your project grows.

## Command surface

```
harnex check [--since <ref>] [--unattended]           # unified validation gate;
                                                      # unattended downgrades
                                                      # unclearable advisory staleness
harnex audit [--plugin-root <path>]                   # generated harness vs. its composition

harnex evidence verify <files...>
harnex evidence record --id <id> [--payload <file|->] # advisory baseline:
                                                      # digests inputs + engine now
harnex telemetry append --kind K --payload <json>
harnex telemetry count --kind K [--since <rfc3339>]
harnex telemetry report [--kind K] [--window 1,7,30,90]

harnex policy permissions generate | audit [--path <p>]
harnex policy versions show | check --tool T --installed V

harnex validate rules <files...>
harnex validate skills <files...> [--scope project|user|plugin]
harnex validate agents <files...>
harnex validate output-styles <files...>
harnex validate settings [<path>]
harnex validate commit-msg <path>                     # closed-enum trailer
harnex validate always-loaded                         # what every session
                                                      # carries, per member

harnex governs resolve <paths...>                     # the rules that are truth
                                                      # about each path
harnex context resolve [paths...]                     # instructions to read;
                                                      # no paths = foundation

harnex session index  [--since <t>] [--project <dir>] [--session <id>]
harnex session facts  [--since <t>] [--with-text]      # counts + citations, no judgement
harnex session submissions [--with-text] [--sample N]  # one entry per instruction, and what followed it
harnex session baseline save --label <name>            # freeze the window; resumes where the last one ended
harnex session baseline diff [--from <a>] [--to <b>]   # rates across two windows, with each window's span
harnex session baseline trend [--project <dir>]        # every window of one scope, side by side

harnex lifecycle routines                             # schedule states: produced |
                                                      # scheduled | overdue |
                                                      # unscheduled | superseded
harnex lifecycle observe --tag T --text X --source S
harnex lifecycle candidates                           # groups past the thresholds,
                                                      # and the ledger they came from
harnex lifecycle observations [--tag T]               # every wording by tag, widest
                                                      # breadth first; closed ones
                                                      # with their decision
harnex lifecycle promote --tag T --text X --decision-text "..."
harnex lifecycle reject  --tag T --text X --decision-text "..."
harnex lifecycle defer   --tag T --text X --decision-text "..."
harnex lifecycle demote  --tag T --text X --decision-text "..."
harnex lifecycle classify --kind K --path P --silence silent|active|unmeasured
harnex lifecycle retire [--window N]
harnex lifecycle decisions [--tag T] [--decision D]

harnex guard hook-event                               # parse stdin hook JSON
harnex guard hook-run <prog> [args...]                # standard hook wrapper
harnex guard hook-stop <prog> [args...]               # Stop hook (always exit 0)
harnex guard stop-audit [--session ID]                # fresh-context Stop audit
harnex guard floor                                    # PreToolUse floor-integrity:
                                                      # blocks hook-skipping git commands
                                                      # and protected-path writes
harnex guard telemetry-emit                           # PostToolUse: record a harness-element
                                                      # invocation (silent, never blocks)

harnex plan audit --plan P [--spec S]                 # spec-workflow review floor:
                  [--baseline B] [--baseline-spec BS] # open C/B rows, vanished rows,
                  [--gates a,b,c]                     # rows added with no round recorded
                                                      # or past what its rounds counted,
                                                      # and firings under undeclared gates

harnex ask serve <page> <asks.json> [--within <min>]  # a decision page on 127.0.0.1: ends on
                 [--no-open]                          # an answer set, taken or stale, or time
harnex ask current <answered.json> <asks.json>        # which earlier answers still hold now
harnex ask words <locale>                             # every sentence serve may put on a page

harnex export schema {config|envelope|finding|event|permissions|error-codes|
                       session|session-submissions|session-baseline|
                       session-trend|asks|ask-outcome|ask-current|ask-words|
                       all}

harnex completions <bash|zsh|fish|powershell|elvish> [--raw]
```

`check --since` limits file-local checks to changed files. A changed or untracked
`harness.toml` revalidates every surface; cross-file checks, including evidence
references, run in full.

`index`, `facts` and `submissions` take the same window: `--since`, `--project`
and `--session`, in any combination. Each emits one JSON envelope carrying the
window's span, coverage, runtime versions and model mix, so a saved envelope is
self-describing — that is the export, and two of them are readable side by side
without the binary having to claim they measured the same work.

`guard stop-audit` allows a clean stop only after a successful critique whose
finding severities are all recognised. A recognised gating finding still
blocks when another item is unreadable; an otherwise unreadable or failed
critique is reported as skipped. Every critique attempt consumes budget,
including failures; a clean verdict resets it. Exhaustion ends automatic
critiques for the session and asks the operator to review the work.

`guard hook-stop` always exits 0, including a spawn failure. Its envelope
records the result or typed error; child output goes to stderr so it cannot
supply a blocking Stop decision.

`context resolve` returns normalized `targets` and `required_instructions` to
read: root `CLAUDE.md` and `.claude/CLAUDE.md`, unconditional or matching rules,
and ancestor `CLAUDE.md` files; `CLAUDE.local.md` follows memory in each
directory. Paths are relative to the directory containing `harness.toml`, even
from a nested working directory; future files are accepted.
No targets asks for the foundation only. Internal target symlinks resolve rules
and memories for both the logical path and its real target. Instruction paths
are canonical and deduplicated at their first occurrence. Ancestor traversal
visits canonical parent directories before their descendants.
Absolute targets, target directories, traversal, targets holding a line
terminator and instructions outside the repository are refused. Broken links
and malformed rules fail instead of returning a partial instruction set. Rule
globs share their grammar with validation and match as Claude Code reads them:
a glob with no `/` but a trailing one matches at every depth, `src/**` is a
`src` directory or file at every depth, and case is folded. A rule whose
`paths:` Claude Code splits, expands or trims otherwise than its syntax says,
or holds a glob the releases of Claude Code's matcher read differently, one
that can match nothing it spells, an escape of a letter, a digit, `\`, `/`,
`?` or whitespace other than a space, or a bracket expression beyond plain
characters and ascending ranges within one kind, fails resolution, and
`check` names a rewrite. `paths-reading-differs` names a rule or skill whose
`paths:` covers other project files once Claude Code drops a trailing `/**`
than as written. The result describes declared
scope, not Claude's runtime loading limits. This is not Claude's memory
runtime: imports, memory exclusions, user memory, output styles, native
`AGENTS.md`, hooks and lifecycle orchestration remain the caller's
responsibility.

`baseline save` records what the window was measured under as well as what it
measured: the build, the paragraph floor, and — where the window was scoped to
a git work tree — the commit the project's harness stood at and whether it had
uncommitted changes. `baseline diff` answers `harness_change` from those, so a
delta across an unchanged harness is not read as the effect of one. What counts
as the harness is what a session in the project loads from its repository —
each directory from the work tree's root down, read with the project's own
settings — an import or a linked `AGENTS.md` included, plus `[session]
harness_paths`, defaulting to where a scaffolded harness lands. Git answers for
these, so a file it ignores does not count, and neither does an import reaching
out of the repository. `baseline trend`
lays every window of one scope side by side, one series per metric, and
subtracts nothing — pairwise comparison, with its overlap and support guards,
stays with `diff`.

`ask serve` puts a decision to the person where a session would otherwise
assume it. The page is any HTML file that keeps the markup promise at the
head of [`script.js`](crates/harness-core/src/ask/script.js): a `fieldset` of
radios per ask, one slot whose content the send controls replace, and the
events the page's own script can hear, among them which asks a refused send is
about. The asks file (`export schema asks`) names what is asked, carries the
caller's version of what each ask shows, and lists the `sources` the page was
made from, read from the working directory.
The page's directory, apart from anything under a dot-name, is served on
127.0.0.1 under a random path, the address goes to stderr and the browser, and
the command ends on one answer set (exit 0), an answer set sent after a source
changed (`stale`, exit 1), or `--within` minutes (`unanswered`, exit 1). Each
answer returns with the version, label and answers its ask showed, so
`ask current` can tell, when the answers are recorded, which still hold
against the asks as they read then. `serve` takes a set asked together whole
or not at all, so a record it writes is never `incomplete`; that comes from a
transport that saves answer by answer, and `awaited` names what its set still
waits on. A page that ships the answers that stand already checked asks only
for the rest. `ask words <locale>` prints, before any page is served, every
sentence the script or a refusal may put on a page in that locale. With the
labels, answers and ids the sentences name, `'` around and `, ` between the
labels of a set's unanswered asks, and digits for counts and the deadline
(`YYYY-MM-DD HH:MM`), that is every character harnex adds, so a page that
carries its own font subset can cover them. A sandboxed session on macOS
binds the port only with `sandbox.network.allowLocalBinding`, or with
`harnex ask serve:*` in `sandbox.excludedCommands`, which runs the command
outside the sandbox when nothing is chained to it; a bare `harnex ask serve`
there matches only the command with no arguments. On Linux the sandbox gives
the command a network of its own, where no browser reaches the page and the
wait runs out, so serve through `excludedCommands` there. On Linux the browser
also receives the address as a command-line argument, which other users of
the machine can read; on a shared host, serve with `--no-open` and open the
address from stderr.

By default every command emits one JSON envelope on stdout; the explicit raw
modes (`export schema --raw`, `completions --raw`) emit the bare artifact for
committing to disk. Exit code: 0 = success, 1 = a gating finding (blocker or
major) or a result that did not pass, 2 = runtime failure.

## What the oracle covers

The `harnex` binary covers the universal Claude Code harness patterns;
the plugin generates the project-native wiring that uses them. Universal
patterns covered out of the box:

- Provenance verification on docs — a rule citing an owner marks it, and the
  gate resolves every marker against the tree, so a rename fails CI instead of
  leaving a rule that points nowhere:

  ```
  [file: Cargo.toml]                                                the file exists
  [file: crates/harness-core/src/path_guard.rs :: fn write_atomic]  and spells that, once
  [file: .claude/rules/making-changes.md § Verification]            and spells that heading, once
  [file: crates/harness-core/src/path_guard.rs:81]                  and that line carries text
  ```

  The anchor is chosen for what it proves. A symbol and a heading are matched
  against what the file spells, so each fails on the rename that removes the
  spelling. A line proves only that the file is that long and the line is not
  blank, so it survives the edit that moved its subject onto another line —
  reserve it for a place inside a body that no name identifies.

- Append-only telemetry with a closed payload schema
- Permission profiles for Claude Code settings: two floors (`baseline` deny,
  `workspace` allow), the tool surfaces (`git-strict`, `gcp-strict`,
  `aws-strict`, `infra-strict`), and one `*-dev` toolchain profile per
  supported language
- Permission rules Claude Code accepts and never consults — a path rule for a
  tool the file permission checks skip, or one naming a tool's primary content
  field — refused at every boundary one can be written
- Claude Code spec compliance (rules / skills / agents / output-styles /
  settings frontmatter)
- `paths:` scope as Claude Code reads it: a glob the releases of its matcher
  read differently is refused, and a rule or skill that covers other files of
  the project once its trailing `/**` is dropped than as written is named with
  a spelling that reads alike both ways
- A character budget over the files the repository puts into every session
  — memory files and their imports, rules whose `paths:` scope nothing, the
  selected output style, each skill, command and agent listing entry — read
  the way Claude Code loads them (`[validate.always_loaded] max_chars`)
- Hook wiring integrity (`harnex audit --plugin-root`) — every
  `${CLAUDE_PROJECT_DIR}` path a hook names that is a scaffold destination
  resolves and the script it spawns directly is executable, so a generated
  handler cannot fail open while the harness reads as wired; a hook the
  project wrote itself is its own to guard
- Generated-artifact integrity — edits inside a managed region, a `copy`
  artifact whose bytes drifted from its template, a fill marker the generating
  step left behind, a `.claude/settings.json` missing a deny rule its profiles
  declare or allowing what one denies
- The enforcement floor (`guard floor`, PreToolUse) — a git command that skips
  the hook stack: `--no-verify` and the prefixes git resolves to it, a
  `core.hooksPath` reroute through `-c` / `--config-env` / the `GIT_CONFIG_*`
  environment, a shell modifier in front of the git word, and any of those
  inside a `$(…)` the line runs or inside a heredoc body it feeds — a body is
  read twice, for what the shell expands into it and for the script its
  receiving program may run. Edit-tool writes to the enforcement surface
  itself are frozen, and a break-glass grant read live from the main checkout
  is how the operator opens them; a Bash write meets the same set in the
  sandbox, and `harnex check` names each path `[guard.floor]` declares that
  neither `sandbox.filesystem.denyWrite` nor the sandbox's own protection
  covers. It is a tripwire, not a boundary. These stay out of
  scope: a second shell handed its script as an argument (`sh -c`), a backtick
  body on the command line, a git alias, a `#` glued
  to the `)` of a process substitution or an array, which is read as a comment
  so the rest of the line goes unread (`<(true)#x; git commit --no-verify`),
  and a word the shell rewrites before git sees it — brace expansion
  (`--no-verify{,}`), a glob standing for the git path (`gi[t]`), and anything
  the check cannot evaluate without running it, so `--no-verify$(true)` and
  `$F` reach git as the flag and pass. The server-side re-run of the same
  gates is the backstop
- The spec-workflow review floor — an open Critical/Blocker row, a row
  deleted, reworded or downgraded instead of gaining its terminal
  disposition, a commit adding finding rows without the decision line that
  records the round that found them, a commit landing rows past what its own
  rounds counted at that rank, a firing under a gate the workflow does not
  have, a committed decision bullet edited instead of appended, and a
  committed section none of them can be read against — each blocks at
  commit (`plan audit`, driven by the shipped `hooks/pre-commit.d/`
  arm)
- Decisions put to the person instead of assumed (`harnex ask`) — any HTML
  page served on 127.0.0.1 takes one answer set; each answer comes back with
  what its ask showed, and `ask current` tells, when it is recorded, whether
  it still holds and what a set asked together still waits on
- Promotion + retirement lifecycle for learnings
- Settings.json hook adapter (the documented hook events)
- Single-command CI gate

Project-specific lint (language ASTs, internal data models, design systems,
package allowlists, multi-phase spec orchestrators) is intentionally out of
scope — that belongs with the project's domain knowledge, not with harnex.

## Enterprise adoption

Organizations rolling harnex out across many repositories drive the plugin
through Claude Code's managed-settings surface so floors are set centrally
and individual repos cannot weaken them. The integration points:

- **Pin the marketplace.** Deploy a `managed-settings.json` with
  `strictKnownMarketplaces` set to `[{"source": "github", "repo":
  "junyeong-ai/harnex"}]` (or your fork). Combined with
  `blockedMarketplaces`, this prevents adoption of unreviewed plugins
  while still allowing harnex.
- **Pin enforced floors.** Set `allowManagedPermissionRulesOnly: true`
  in managed settings so ONLY managed-scope permission rules are honored —
  user / project / local permission rules are then ignored, not merged. To make
  the `baseline` deny a non-removable floor under this policy, DEPLOY that deny
  set in the managed settings itself; a deny shipped only in a project's
  `.claude/settings.json` under `permissions.deny` would be ignored. Without this
  policy, rules from all scopes merge and the project deny applies.
- **Pin behavioral guidance.** The managed `claudeMd` key carries the
  organization-wide instructions delivered before any project CLAUDE.md
  ("Always run `make lint` before committing", compliance reminders).
  This survives `claudeMdExcludes` at every other scope.
- **Check project customization before locking it.**
  `strictPluginOnlyCustomization: ["skills", "hooks"]` blocks user and
  project skills and hooks. The harnex plugin still loads, but the hooks and
  skills it generates into a target repository are project customization and
  are blocked. Deploy those controls through managed settings or an approved
  organization plugin before enabling this lock.
- **Disable skill shell injection.** Set `disableSkillShellExecution:
  true` in managed settings to neutralise `` !`<command>` `` substitution
  in user / project / plugin / additional-directory skills (bundled and
  managed skills are exempt). harnex's templates do not rely on
  shell-injection, so it remains fully functional under this policy.

See `https://code.claude.com/docs/en/managed-settings` for how managed
settings are delivered per OS (`managed-settings.d/`, plist, registry, Group
Policy) and `https://code.claude.com/docs/en/settings-reference` for the keys.

## Development

Use `rust-toolchain.toml` for the build toolchain; `Cargo.toml` declares the
minimum supported Rust version. `.github/workflows/ci.yml` defines the gates.

Browser tests live in `crates/harness-cli/tests/ask-page`. Use the Node
version in `.node-version`, build the CLI with `cargo build --locked`, then
run `npm ci`, `npx playwright install chromium firefox webkit`, and `npm test`
from that directory. The lockfile pins Playwright and its browser revisions.

## Operating context

Day-to-day operation is delegated to Claude Code. See `CLAUDE.md` and
`.claude/rules/` for the AI operating context. This README is the only
file written for humans; everything else under this repo is consumed
directly by Claude.

## License

MIT. See [`LICENSE`](LICENSE).
