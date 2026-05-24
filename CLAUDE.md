# harnex

Harness engineering for Claude Code projects. Two surfaces: the **harnex
plugin** (a skill that generates project-native harness tooling) and the
**`harness` binary** (the Pure-Rust, deterministic, no-network oracle the
plugin's templates are verified against).

## The plugin (primary surface)

| Path | Responsibility |
|---|---|
| `SKILL.md` | single-skill plugin entry; 4 modes (scaffold / extend / audit / regenerate) |
| `reference/` | L1 knowledge — spec-facts, enforced-vs-advisory, keep-soften-cut, language-matrix, exploration |
| `templates/` | L2 deterministic safety-critical templates (`common` + `typescript` + `python`) |
| `.claude-plugin/plugin.json` | manifest; `version` omitted (commit SHA drives updates) |

Generated files land in `${CLAUDE_PROJECT_DIR}`; bundled assets are referenced
via `${CLAUDE_PLUGIN_ROOT}`. The skill composes templates — it never
free-generates safety-critical code.

## Where things live (oracle binary)

| Module (crate path) | Responsibility |
|---|---|
| `harness-core::config` | `harness.toml` load + cross-section validate (`MetaConfig`, `CodegenGroupDecl`, `RetirementExemptDecl`, …) |
| `harness-core::envelope` | `Finding`, `Severity`, `Location`, list response |
| `harness-core::error` | `Error` + `ErrorCode` (stable wire codes) |
| `harness-core::path_guard` | safe write paths: `write_atomic` (full replace) + `append_line` (ledgers) |
| `harness-core::evidence` | provenance verifier (4 strategies) |
| `harness-core::telemetry` | JSONL ledger with closed payload schema; `StorageKind` strategy enum |
| `harness-core::codegen` | sentinel-block source → target sync (3 renderers) |
| `harness-core::policy` | permission profiles (baseline, git-strict, gcp-strict, aws-strict, rust-dev, python-dev, typescript-dev) + version pins |
| `harness-core::validate` | rule / skill / settings / commit-msg checks |
| `harness-core::lifecycle` | observation + decision ledger + retirement |
| `harness-core::guard` | Claude Code hook adapter + Stop auditor |
| `harness-core::export` | JSON Schema emission |
| `harness-core::graph` | read-only `nodex` CLI bridge |
| `harness-core::check` | unified validation gate |
| `harness-cli` | thin clap wrapper; each command emits one envelope |

## Documentation map

- `SKILL.md` + `reference/` + `templates/` — the harnex plugin (consumed by
  Claude Code when the plugin is installed, not by this repo's own sessions).
- `README.md` — the only human-facing surface (the two surfaces, install,
  oracle quickstart, what the oracle covers).
- `.claude/rules/constitution.md` — always-loaded project laws (Articles I–VIII).
- `.claude/rules/<topic>.md` — path-scoped guidance; loaded automatically
  when you read files matching that rule's `paths:` frontmatter.
- `crates/<crate>/CLAUDE.md` — crate-scoped guidance; loaded when you
  work inside that crate.
- `schemas/harness.schema.json` — JSON Schema for `harness.toml` (regen
  via `harness export schema config --raw`).

For the full command surface, run `harness --help` or read `README.md`.

## What this project refuses to do

- No async runtime, no servers, no daemons, no network at command time.
- No project domain vocabulary in source — every project-specific shape
  derives from `harness.toml` declarations.
- No string-matched errors — typed `Error` + stable `ErrorCode`.
- No backward-compatibility shims — rename in place, delete legacy in the
  same commit.
- No `docs/` directory — `README.md` is the single human surface;
  everything else under this repo is consumed by Claude.
- No `chrono`, no `time`, no `once_cell` — `jiff` + `std::sync::LazyLock`
  are the chosen primitives (see `.claude/rules/jiff-time.md`).
