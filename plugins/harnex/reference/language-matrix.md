# Language / toolchain matrix (deterministic parameterization)

How harnex detects a project's stack and parameterizes the templates. Detection
is from build/lock/workspace files — structural signals, never heuristic
guessing. Never cross-wire a language (a TS repo gets biome, a Python repo gets
ruff; emitting the wrong formatter is the meta-failure to avoid).

## Detection fingerprint (read manifests + lockfiles first)

**Match every row whose signal is present.** Detection answers with a set, not
a winner: a lockfile is evidence that a stack is here, never evidence that it
is the only one, and the language tier is emitted once per match. Whether a
matched language is a monorepo or single-package is a sub-distinction
(workspace globs present → monorepo + Phase-3 fan-out; absent → lean
single-package scaffold), NOT a fallback that swallows an unrecognized stack.

Row order is presentation, never precedence. Resolving two present stacks by
which row comes first is a tiebreak wearing a decision's clothes, and it fails
loudest exactly where it matters most: a measured repository here carries
`pnpm-lock.yaml` beside `uv.lock` with 17,085 `.py` files against 3,433 `.ts`,
so first-match-wins would wire `biome` as its formatter, grant `pnpm` and never
`uv`, and skip 83% of the source in silence. That is the wrong-profile
meta-failure arriving through the front door.

| Signal | Stack |
|---|---|
| `pnpm-lock.yaml` + `package.json` (`pnpm-workspace.yaml`) | TypeScript / pnpm (+ Turborepo if `turbo.json`) |
| `uv.lock` + `pyproject.toml` (`[tool.uv.workspace]`) | Python / uv (+ Just if `Justfile`, prek if `.pre-commit-config.yaml`) |
| `Cargo.toml` (`[workspace]`) + `Cargo.lock` | Rust / cargo |
| `settings.gradle{,.kts}` or `build.gradle{,.kts}` (+ `gradlew`) | JVM / Gradle (+ version catalog if `gradle/libs.versions.toml`) |
| `pom.xml` (+ `mvnw`) | JVM / Maven |
| none of the above (e.g. `go.mod`, `*.csproj`, `Gemfile`, `composer.json`) | **no profile** — foundation tier only |

Nothing arbitrates between two matched stacks at runtime either: each
`hooks/post-format-<lang>.sh` dispatches on the file extension and exits 0 on
anything it does not own, so they coexist as separate PostToolUse entries.
`permissions.allow` unions both toolchains, and each stack gets its own
`<lang>-conventions.md`.

**No profile is a first-class outcome, and it is not a refusal.** The
composition manifest (`templates/scaffold.toml`) splits a harness into a
`foundation` tier that needs no language and a `language` tier that does, so a
stack with no profile receives the whole enforced floor — the permission deny
set AND the permission allow floor, the foundation rules, `harness.toml`, the
hook wrappers, the gitleaks pre-commit hook — and is told exactly which
language-tier artifacts are missing and why. What must never happen is a
*wrong* profile: emitting ruff into a Go repo is the meta-failure this matrix
exists to prevent. An absent profile is a different thing, and withholding a
floor the stack never needed a profile for protects nobody. Offer
`extend language <lang>` as the way to close the remaining tier.

## Gate-driver detection (evidence, not a per-language default)

A project's gates run through a task runner more often than through the
language toolchain directly, and which runner is a project fact, not a
language one. The signal below says a runner is present; the grant comes from
how the project actually **invokes** it, read from the same CI and task files
Step 1 already reads for `## Build & test`.

The invocation is the part that decides. `uv run poe lint` is already covered
by `Bash(uv *)`, so a project that only ever wraps its runner needs no new
rule, while one that also types `poe lint` does. Emit the grant only when no
existing rule already matches the observed form — a grant that can never fire
is a no-op, and harnex refuses those for the same reason it refuses an allow
rule for `ls`.

| Signal | Grant |
|---|---|
| `Justfile` / `justfile` | `Bash(just *)` |
| `Makefile` | `Bash(make *)` |
| `[tool.poe.tasks]` in `pyproject.toml` | `Bash(poe *)` |
| `[tool.hatch.envs.*.scripts]` in `pyproject.toml` | `Bash(hatch *)` |
| `[tool.pdm.scripts]` in `pyproject.toml` | `Bash(pdm *)` |
| `scripts` in `package.json` | already covered by the package manager grant |
| `Taskfile.yml` | `Bash(task *)` |
| `mise.toml` / `.mise.toml` `[tasks]` | `Bash(mise *)` |

**No signal, no grant.** A runner the project does not declare is the same
guess as the wrong formatter. Two or more signals means two or more grants —
a repo with a `Justfile` wrapping `poe` prompts on whichever one it types.

A `<lang>-dev` profile is the ecosystem's mainstream toolchain and is not a
claim about which of it this project uses; `python-dev` carries `ty`, `mypy`
and `pyright` because a grant for an absent tool never matches while a missing
one prompts on every invocation. The gate driver is the part that must be
observed, because no ecosystem default covers it.

**The JVM row is one profile for two languages, on purpose.** The axis a
template directory is keyed on is the toolchain, not the language name:
`typescript/` is really node+pnpm and `python/` is uv. On the JVM, Gradle and
Maven each serve Java and Kotlin, and no build-file fingerprint separates them
— `build.gradle.kts` names the DSL, not the sources. Mixed Java + Kotlin trees
are the common case (Android, an in-progress migration), so a `java/`-only
profile would leave every `.kt` file unformatted while reporting a complete
scaffold. `jvm/post-format.sh` dispatches on the file extension instead, which
is what invariant 5 actually asks for: `.java` reaches a Java formatter and
`.kt` a Kotlin one, never each other's.

## Per-language template parameters

| Axis | TypeScript (pnpm) | Python (uv) | Rust (cargo) | JVM (Gradle / Maven) |
|---|---|---|---|---|
| Formatter (PostToolUse) | `biome check --write` | `ruff format` + `ruff check --fix` | `rustfmt <file>` (+ `rustfmt.toml`, below) | `google-java-format -i` on `.java`, `ktlint -F` on `.kt`/`.kts` — never via the build tool |
| Typecheck | `tsc` (via `turbo run type-check`) | `ty` / `mypy` / `pyright` — whichever the project configures | `cargo check` | `./gradlew compileJava compileKotlin` / `./mvnw -o compile` |
| Verifier forms the runner dispatches | `.sh` + `.ts` via `node` | `.sh` + `.py` via `uv run --frozen` | `.sh` only (no per-hook `.rs` build); JSON parsed with `jq` | `.sh` only (no per-hook JVM start); JSON parsed with `jq` |
| Gate runner (when the project declares none) | `pnpm` (+ `turbo`) | `uv run` (hooks via `prek`) | `cargo` | `./gradlew` / `./mvnw` (wrapper first) |
| Secret scan | gitleaks | gitleaks | gitleaks | gitleaks |
| PreToolUse default | non-blocking (advisory) | project choice (blocking convention-gate is valid) | non-blocking | non-blocking |

**JVM formatting never routes through the build tool.** `gradlew spotlessApply`
and `mvn spotless:apply` start a JVM, warm a daemon, and format the whole
project; at PostToolUse that exceeds the hook timeout on every single edit, and
a formatter that times out is a formatter that silently does nothing. The
standalone CLIs finish inside the budget a per-edit hook has. Each arm also
probes its own formatter and skips when absent, so a Java-only or Kotlin-only
repo is never penalised for the tool it does not install.

## Language-agnostic constants (every generated harness)

- `autoMemoryEnabled: false` is a defensible default for team repos (shared
  context lives in git, not per-developer caches) — emit only if the project
  signals it; never force.
- Two Claude Code hook wrappers, both language-agnostic: `_runner.sh` (anchor
  cwd at the project root — `${CLAUDE_PROJECT_DIR}`, else git asked about the
  wrapper's own directory, else that directory's parent; never a probe of the
  working directory, which the runtime does not promise and which names the
  inner repository inside a submodule → dispatch by verifier extension) and
  `_stop_runner.sh` (same, always exit 0). Both reject `..` path-traversal in
  the script-name argument. The wrapper probes no toolchain — each non-shell dispatch arm
  probes the interpreter it invokes, and a `.sh` verifier probes whatever it
  shells out to. A wrapper that gated on the language's build tool skipped
  working hooks whenever an unrelated tool was missing.
- Two git hooks, activated by `git config core.hooksPath hooks`:
  `hooks/pre-commit` runs gitleaks on staged changes (the enforced half of
  "secrets never reach git"; permission deny covers only Claude — fail-open if
  gitleaks is absent, and a scan that fails rather than finds blocks as
  unscanned, since the two exit alike unless findings are given a code of
  their own; wants gitleaks 8.19+, whose `git` subcommand it scans with;
  escape hatch `HARNEX_SKIP_GITLEAKS=1`), then dispatches
  every executable in `hooks/pre-commit.d/` in name order — the seam patterns
  add commit gates through, since the hook's own copy is held byte-identical.
  `hooks/commit-msg` validates the trailers `[validate.commit_msg]` declares,
  fail-open until configured.
- Two permission floors, both foundation-tier and neither language-dependent:
  `templates/common/permissions.deny.json` (the oracle's `baseline` profile)
  and `templates/common/permissions.allow.json` (its `workspace` profile —
  `Edit`, `Write`, `mkdir -p`, the mutating git verbs, and `harnex`, which the
  generated `governance.md` sends its reader to on every loop pass). Copy both
  verbatim; the language tier merges its toolchain grants beside them as a set
  union, never a replacement.
- `permissions.deny` floor: do NOT hand-write or re-enumerate it — copy
  `templates/common/permissions.deny.json` verbatim. That file is the single
  source of truth (generated from the oracle's `baseline` profile, held in sync
  by the `policy_template_sync` test). By category it covers: sensitive-file
  reads plus writes/edits, destructive git, `rm -rf` of roots, destructive
  `find`, arbitrary code execution, `sudo`, `chmod -R 777`. Sensitive-file
  patterns are precise file shapes (extensions, the `secrets/` dir, credential
  JSON, home credential paths), never broad substrings that would hard-block
  source files. A Read deny already blocks `cat`/`head`/`tail`/`sed` of the
  same path. Cloud profiles (`gcp-strict`, `aws-strict`) add their destroy
  verbs. Listing the individual rules anywhere but the SSoT is how it drifts —
  don't.
- `<lang>/permissions.allow.json` carries the language toolchain and nothing
  else — the floor above is where `Edit`/`Write` and git live, so the two sets
  are disjoint and a no-profile stack still gets one. Grant only commands that
  actually prompt. Read-only built-ins
  (`ls`, `grep`, `cat`, read-only `git`) never prompt, so an allow rule for them
  is a no-op — never emit one. Broad env-runners (`npx *`) are excluded; scope
  them per project (`npx <tool> *`).
- `constitution.md` is the one `.claude/rules/*.md` that omits `paths:`
  (foundation, always-loaded). Every other rule carries `paths:`.
- **The per-file formatter must resolve the same config the gate does.**
  A PostToolUse hook formats one file; the CI gate formats the workspace.
  Where the two read configuration differently, every edit reverts what the
  gate requires and the loop is invisible until CI reds. Rust is the live
  case: `rustfmt <file>` never sees `Cargo.toml` and defaults to edition
  2015, so a Rust scaffold emits `rustfmt.toml` carrying the edition read
  from the project's manifest. Check the same property before wiring any new
  language's formatter.
- Hook config `timeout` in SECONDS (10–30 typical), `type: "command"`.
- Sentinel-block codegen source may be toml/json/yaml (`source_format`) — point
  at the project's existing SSoT, never hand-maintain a duplicate.

## Monorepo exploration (divide-and-conquer)

Before generating into a brownfield monorepo:
1. Enumerate modules/languages/toolchains deterministically from workspace +
   lock + manifest files. Read those first, never the whole repo.
2. Fan out one read-only Explore agent per **independent** module (clean
   boundaries, no bidirectional deps), each with an explicit objective, output
   format, and scope boundary; write results to a structured module-map
   artifact, not back through the orchestrator's context.
3. Synthesize from the artifact with a single agent. Generation is single-agent
   and sequential — fan-out is for read-heavy exploration only (it costs
   4–15× tokens and hurts on dependent/stateful work).
