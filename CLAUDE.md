# harness-toolkit

Harness engineering toolkit for Claude Code projects. Pure-Rust workspace,
JSON-envelope CLI, deterministic + no-network.

## Where things live

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
| `harness-core::init` | project scaffolder + hook script generation |
| `harness-core::graph` | read-only `nodex` CLI bridge |
| `harness-core::check` | unified validation gate |
| `harness-cli` | thin clap wrapper; each command emits one envelope |

## Documentation map

- `README.md` — the only human-facing surface (install, quickstart, IDE
  integration, porting tables).
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
