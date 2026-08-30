# harnex

Harness engineering for Claude Code projects. harnex has two surfaces:

- **The harnex plugin** (primary) — a Claude Code skill that *generates*
  project-fit, project-native harness tooling (hooks, `settings.json`,
  `CLAUDE.md`, path-scoped rules) into a target repo, in that repo's own
  language, from verified spec-correct templates. The value is the
  knowledge of getting the Claude Code spec right, distributed as a skill —
  not a runtime you depend on.
- **The `harnex` binary** (oracle) — a Pure-Rust, JSON-first CLI that
  deterministically verifies a harness: provenance, closed-schema telemetry,
  lifecycle, runtime guards, a unified validation gate. It is the
  spec-correct reference the plugin's templates are checked against.

## Why

Modern Claude follows in-context conventions well. What it cannot do alone
is keep its harnex spec-correct as the upstream surface evolves, enforce
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
it they say so and stop rather than estimating from the logs.

It also comes last. `measure` answers whether a harness earns its place and
what the operator repeats every session, so it has something to say once there
is a harness and a few weeks of transcripts — `scaffold` is where a project
without one starts.

It detects the stack from lockfile + manifest (TypeScript/pnpm, Python/uv,
Rust/cargo, JVM/Gradle-Maven for Java and Kotlin) and composes the harness
from `templates/scaffold.toml`, which declares every artifact in two tiers: a
**foundation** tier with no language dependency, and a **language** tier that
needs a detected profile. The manifest is the only list of what a harness
contains — read it rather than a summary. A stack harnex has no profile for
still receives the foundation tier and a report of what is missing, and a repo
holding two stacks receives the language tier once per stack. It never
free-generates a hook or permission rule. Knowledge lives in
`reference/` (the spec facts, the enforced-vs-advisory split, the
keep/soften/cut principle, the language matrix, the exploration procedure).

## The oracle binary

```bash
curl -fsSL https://github.com/junyeong-ai/harnex/raw/main/scripts/install.sh | bash
```

Takes the binary this project releases for your platform, verifies its sha256,
and installs it to `~/.local/bin` — no Rust toolchain involved. Linux archives
link musl statically, so one per architecture runs on any distribution.

macOS and Linux, on x86-64 and arm64, have a release binary. Anywhere else the
installer says so and builds from source instead.

```bash
scripts/install.sh --version v1.2.3   # a specific release rather than the latest
scripts/install.sh --build            # build from source instead of downloading
scripts/install.sh --check            # what is installed, changing nothing
scripts/install.sh --help             # every option
```

`--build` needs Rust 1.98+; the script reads that floor from `Cargo.toml`
rather than holding a second copy, and `rust-toolchain.toml` pins the exact
toolchain so a checkout builds with the same compiler CI uses. `cargo build
--release` still works for a local build without installing.

Every asset is built by `.github/workflows/release.yml` from the tag it is
attached to, and carries provenance saying so:

```bash
gh attestation verify harnex-<target>.tar.gz --repo junyeong-ai/harnex
```

Installing the plugin does not install the binary. Enabling a plugin is not
consent to put an executable on the machine, so the two are separate acts and
the plugin reports the oracle as missing rather than fetching it.

## IDE integration

`schemas/harness.schema.json` ships in this repo. Point your TOML
language server at it for autocomplete + validation on `harness.toml`:

- **Taplo / VS Code Even-Better-TOML**: the generated `harness.toml`
  includes a `#:schema <url>` directive at the top — replace
  `<owner>/<repo>` with your fork's path, or use a `file://` URL of
  `schemas/harness.schema.json` in your local checkout.
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
./harnex check
./harnex check --fix      # auto-fix what can be fixed (currently: codegen sync)
```

`examples/harness.toml.minimal` enables just evidence (provenance verifier)
and telemetry (event ledger) — the smallest useful surface.
`examples/harness.toml.team` is the full-surface config (adds
validate.rules/skills, policy.permissions, lifecycle, codegen, …). Start
from one and extend with `[[kinds]]`, `[[lifecycle.consumer_detectors]]`,
`[[codegen.groups]]`, `[[policy.versions]]`, `[validate.commit_msg]` as your
project grows.

## Command surface

```
harnex check [--since <ref>] [--fix]                  # unified validation gate
harnex audit [--plugin-root <path>]                   # generated harness vs. its composition

harnex evidence verify <files...>
harnex telemetry append --kind K --payload <json>
harnex telemetry count --kind K [--since <rfc3339>]
harnex telemetry report [--kind K] [--window 1,7,30,90]

harnex codegen sync | check

harnex policy permissions generate | audit [--path <p>]
harnex policy versions show | check --tool T --installed V

harnex validate rules <files...>
harnex validate skills <files...>
harnex validate agents <files...>
harnex validate output-styles <files...>
harnex validate settings [<path>]
harnex validate commit-msg <path>                     # closed-enum trailer

harnex session index  [--since <t>] [--project <dir>] [--session <id>]
harnex session facts  [--since <t>] [--with-text]      # counts + citations, no judgement
harnex session submissions [--with-text] [--sample N]  # one entry per instruction, and what followed it
harnex session baseline save --label <name>            # freeze the window; resumes where the last one ended
harnex session baseline diff [--from <a>] [--to <b>]   # rates across two windows, with each window's span

harnex lifecycle observe --tag T --text X --source S
harnex lifecycle candidates
harnex lifecycle promote --tag T --text X --decision-text "..."
harnex lifecycle reject  --tag T --text X --decision-text "..."
harnex lifecycle defer   --tag T --text X --decision-text "..."
harnex lifecycle demote  --tag T --text X --decision-text "..."
harnex lifecycle classify --kind K --path P [--silent]
harnex lifecycle retire [--window N]
harnex lifecycle decisions [--tag T] [--decision D]

harnex guard hook-event                               # parse stdin hook JSON
harnex guard hook-run <prog> [args...]                # standard hook wrapper
harnex guard hook-stop <prog> [args...]               # Stop hook (always exit 0)
harnex guard stop-audit [--session ID]                # fresh-context Stop audit

harnex plan audit --plan P [--spec S]                 # spec-workflow review floor:
                  [--baseline B] [--baseline-spec BS] # open C/B rows, vanished rows,
                                                      # decision-log convergence

harnex graph version | backlinks <id> | orphans | stale | nodes --kind K | diff <a> <b>

harnex export schema {config|envelope|finding|event|permissions|error-codes|
                       session|session-submissions|session-baseline|all}

harnex completions <bash|zsh|fish|powershell|elvish> [--raw]
```
`index`, `facts` and `submissions` take the same window: `--since`, `--project`
and `--session`, in any combination. Each emits one JSON envelope carrying the
window's span, coverage, runtime versions and model mix, so a saved envelope is
self-describing — that is the export, and two of them are readable side by side
without the binary having to claim they measured the same work.

`baseline save` records what the window was measured under as well as what it
measured: the build, the paragraph floor, and — where the window was scoped to
a git work tree — the commit the project's harness stood at and whether it had
uncommitted changes. `baseline diff` answers `harness_change` from those, so a
delta across an unchanged harness is not read as the effect of one. What counts
as the harness is `[session] harness_paths`, defaulting to what Claude Code
reads.

By default every command emits one JSON envelope on stdout; the explicit raw
modes (`export schema --raw`, `completions --raw`) emit the bare artifact for
committing to disk. Exit code: 0 = success, 1 = gating finding (blocker or
major), 2 = runtime failure.

## What the oracle covers

The `harnex` binary covers the universal Claude Code harness patterns;
the plugin generates the project-native wiring that uses them. Universal
patterns covered out of the box:

- Provenance verification on docs — a rule citing an owner marks it, and the
  gate resolves every marker against the tree, so a rename fails CI instead of
  leaving a rule that points nowhere:

  ```
  [file: crates/harness-core/src/path_guard.rs:81]   file must exist and hold that line
  [file: pyproject.toml]                             file must exist
  ```

- Append-only telemetry with a closed payload schema
- Sentinel-block enum codegen across many files
- Permission profiles for Claude Code settings: two floors (`baseline` deny,
  `workspace` allow), the tool surfaces (`git-strict`, `gcp-strict`,
  `aws-strict`), and one `*-dev` toolchain profile per supported language
- Permission rules Claude Code accepts and never consults — a path rule for a
  tool the file permission checks skip, or one naming a tool's primary content
  field — refused at every boundary one can be written
- Claude Code spec compliance (rules / skills / agents / output-styles /
  settings frontmatter)
- Hook wiring integrity — every `${CLAUDE_PROJECT_DIR}` path a hook names
  resolves and the script it spawns directly is executable, so a handler
  cannot fail open while the harness reads as wired
- Generated-artifact integrity — edits inside a managed region, a `copy`
  artifact whose bytes drifted from its template, a fill marker the generating
  step left behind
- The spec-workflow review floor — an open Critical/Blocker row, a row
  deleted, reworded or downgraded instead of gaining its terminal
  disposition, a decision log whose Critical+Blocker count will not fall, and
  a committed decision bullet edited instead of appended each block at commit
  (`plan audit`, driven by the shipped `hooks/pre-commit.d/` arm)
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
- **Pin enforced floors.** Set `permissions.allowManagedPermissionRulesOnly: true`
  in managed settings so ONLY managed-scope permission rules are honored —
  user / project / local permission rules are then ignored, not merged. To make
  the `baseline` deny a non-removable floor under this policy, DEPLOY that deny
  set in the managed settings itself; a deny shipped only in a project's
  `permissions.deny.json` would be ignored. (Without this policy, rules from all
  scopes merge and the project deny applies.)
- **Pin behavioral guidance.** The managed `claudeMd` key carries the
  organization-wide instructions delivered before any project CLAUDE.md
  ("Always run `make lint` before committing", compliance reminders).
  This survives `claudeMdExcludes` at every other scope.
- **Optional hard-lock plugin surface.** Set
  `strictPluginOnlyCustomization: ["skills", "hooks"]` to require that
  every skill or hook be plugin-managed (not freely added at user /
  project scope). harnex stays usable because its content ships as a
  plugin; everything else routes through the marketplace.
- **Disable skill shell injection.** Set `disableSkillShellExecution:
  true` in managed settings to neutralise `` !`<command>` `` substitution
  in user / project / plugin / additional-directory skills (bundled and
  managed skills are exempt). harnex's templates do not rely on
  shell-injection, so it remains fully functional under this policy.

See `https://code.claude.com/docs/en/settings` for the complete managed
settings surface and OS-specific deployment paths (`managed-settings.d/`,
plist, registry, Group Policy).

## Operating context

Day-to-day operation is delegated to Claude Code. See `CLAUDE.md` and
`.claude/rules/` for the AI operating context. This README is the only
file written for humans; everything else under this repo is consumed
directly by Claude.

## License

MIT. See [`LICENSE`](LICENSE).
