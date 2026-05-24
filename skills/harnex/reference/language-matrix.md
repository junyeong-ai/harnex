# Language / toolchain matrix (deterministic parameterization)

How harnex detects a project's stack and parameterizes the templates. Detection
is from build/lock/workspace files — structural signals, never heuristic
guessing. Never cross-wire a language (a TS repo gets biome, a Python repo gets
ruff; emitting the wrong formatter is the meta-failure to avoid).

## Detection fingerprint (read manifests + lockfiles first)

| Signal | Stack |
|---|---|
| `pnpm-lock.yaml` + `package.json` (`pnpm-workspace.yaml`) | TypeScript / pnpm (+ Turborepo if `turbo.json`) |
| `uv.lock` + `pyproject.toml` (`[tool.uv.workspace]`) | Python / uv (+ Just if `Justfile`, prek if `.pre-commit-config.yaml`) |
| `Cargo.toml` (`[workspace]`) + `Cargo.lock` | Rust / cargo |
| manifest present, no workspace globs | single-package (non-monorepo) → lean scaffold |

## Per-language template parameters

| Axis | TypeScript (pnpm) | Python (uv) | Rust (cargo) |
|---|---|---|---|
| Formatter (PostToolUse) | `biome check --write` | `ruff format` + `ruff check --fix` | `cargo fmt` |
| Typecheck | `tsc` (via `turbo run type-check`) | `ty` | `cargo check` |
| Hook runner inner cmd | native `node` on `.ts` | `uv run --frozen python -m <mod>` | direct binary |
| Gate runner | `pnpm` (+ `turbo`) | `just` (hooks via `prek`) | `cargo` |
| Secret scan | gitleaks | gitleaks | gitleaks |
| PreToolUse default | non-blocking (advisory) | project choice (blocking convention-gate is valid) | non-blocking |

## Language-agnostic constants (every generated harness)

- `autoMemoryEnabled: false` is a defensible default for team repos (shared
  context lives in git, not per-developer caches) — emit only if the project
  signals it; never force.
- Two hook wrappers: `_runner.sh` (anchor cwd at git root → probe runtime →
  fail-open on env drift → dispatch) and `_stop_runner.sh` (same, always exit
  0). Both reject `..` path-traversal in the script-name argument.
- `permissions.deny` secret block: `.env*`, `*.key`, `*.pem`, `*credentials*`,
  `*secret*` (Read + Write + `Bash(cat ...)`); destructive: `git push --force`,
  `git reset --hard`, `git add .`/`-A`/`-u`, `rm -rf` of roots, `find -exec`,
  arbitrary code exec (`node -e`, `python3 -c`), `sudo`, `chmod -R 777`. Cloud
  profiles add their destroy verbs.
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
