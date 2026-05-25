# Language / toolchain matrix (deterministic parameterization)

How harnex detects a project's stack and parameterizes the templates. Detection
is from build/lock/workspace files — structural signals, never heuristic
guessing. Never cross-wire a language (a TS repo gets biome, a Python repo gets
ruff; emitting the wrong formatter is the meta-failure to avoid).

## Detection fingerprint (read manifests + lockfiles first)

Match the FIRST supported-language row. Whether the matched language is a
monorepo or single-package is a sub-distinction (workspace globs present →
monorepo + Phase-3 fan-out; absent → lean single-package scaffold), NOT a
fallback that swallows an unrecognized stack.

| Signal | Stack |
|---|---|
| `pnpm-lock.yaml` + `package.json` (`pnpm-workspace.yaml`) | TypeScript / pnpm (+ Turborepo if `turbo.json`) |
| `uv.lock` + `pyproject.toml` (`[tool.uv.workspace]`) | Python / uv (+ Just if `Justfile`, prek if `.pre-commit-config.yaml`) |
| `Cargo.toml` (`[workspace]`) + `Cargo.lock` | Rust / cargo |
| none of the above (e.g. `go.mod`, `pom.xml`, `build.gradle`, `Gemfile`, `composer.json`) | **UNSUPPORTED** — refuse |

**Unsupported stack is a first-class outcome, not undefined behavior.** When no
supported-language row matches, harnex must NOT emit a half-built lean scaffold
with no language profile. Refuse explicitly: write nothing, and tell the
operator "harnex supports typescript / python / rust; detected <stack> is not
supported — add a language (`extend language <lang>`) or scaffold the
language-agnostic `common/` pieces by hand." A wrong-or-empty profile is worse
than an honest refusal.

## Per-language template parameters

| Axis | TypeScript (pnpm) | Python (uv) | Rust (cargo) |
|---|---|---|---|
| Formatter (PostToolUse) | `biome check --write` | `ruff format` + `ruff check --fix` | `rustfmt <file>` |
| Typecheck | `tsc` (via `turbo run type-check`) | `ty` | `cargo check` |
| Hook runner inner cmd | native `node` on `.ts` | `uv run --frozen python -m <mod>` | probe `cargo`, dispatch `.sh` (no per-hook `.rs` build); JSON hooks use `jq` |
| Gate runner | `pnpm` (+ `turbo`) | `just` (hooks via `prek`) | `cargo` |
| Secret scan | gitleaks | gitleaks | gitleaks |
| PreToolUse default | non-blocking (advisory) | project choice (blocking convention-gate is valid) | non-blocking |

## Language-agnostic constants (every generated harness)

- `autoMemoryEnabled: false` is a defensible default for team repos (shared
  context lives in git, not per-developer caches) — emit only if the project
  signals it; never force.
- Two Claude Code hook wrappers: `_runner.sh` (anchor cwd at git root →
  probe runtime → fail-open on env drift → dispatch) and `_stop_runner.sh`
  (same, always exit 0). Both reject `..` path-traversal in the script-name
  argument.
- One git hook: `hooks/pre-commit` runs gitleaks on staged changes (the
  enforced half of "secrets never reach git"; permission deny covers only
  Claude). Fail-open if gitleaks is absent; escape hatch via
  `HARNEX_SKIP_GITLEAKS=1`. Activated by `git config core.hooksPath hooks`.
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
- `<lang>/permissions.allow.json` grants only commands that actually prompt
  (`Edit`/`Write`, mutating git, the language toolchain). Read-only built-ins
  (`ls`, `grep`, `cat`, read-only `git`) never prompt, so an allow rule for them
  is a no-op — never emit one. Broad env-runners (`npx *`) are excluded; scope
  them per project (`npx <tool> *`).
- `constitution.md` is the one `.claude/rules/*.md` that omits `paths:`
  (foundation, always-loaded). Every other rule carries `paths:`.
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
