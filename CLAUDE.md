# harnex

Harness engineering for Claude Code projects. Two surfaces: the **harnex
plugin** (a skill that generates project-native harness tooling) and the
**`harnex` binary** (the Pure-Rust, deterministic, no-network oracle the
plugin's templates are verified against).

## The plugin (primary surface)

`.claude-plugin/marketplace.json` (repo root) is the marketplace; the plugin
lives under `plugins/harnex/`:

| Path | Responsibility |
|---|---|
| `plugins/harnex/SKILL.md` | single-skill plugin entry; modes: scaffold / extend / retire / audit / regenerate |
| `plugins/harnex/commands/` | user-invoked procedures (`/harnex:measure`) — outside the skill and its budget |
| `scripts/install.sh` | installs the oracle — the released binary first, source on request; the plugin never does, and reports it missing instead |
| `.github/workflows/release.yml` | builds every asset that installer downloads; `release_install_sync` holds the two to one set of targets and one asset name |
| `plugins/harnex/agents/` | sub-agents those procedures dispatch; `model` is the cost lever |
| `plugins/harnex/reference/` | L1 knowledge — spec-facts, enforced-vs-advisory, keep-soften-cut, language-matrix, exploration |
| `plugins/harnex/templates/` | L2 deterministic safety-critical templates (`common` + per-language) |
| `plugins/harnex/templates/scaffold.toml` | composition manifest — every artifact a harness contains, its tier, destination, and merge/managed flags (skill + fixture test + audit coverage all read it) |
| `plugins/harnex/.claude-plugin/plugin.json` | manifest; `version` omitted (commit SHA drives updates) |

Generated files land in `${CLAUDE_PROJECT_DIR}`; bundled assets are referenced
via `${CLAUDE_SKILL_DIR}` (the documented, install-level-portable anchor). The
skill composes templates — it never free-generates safety-critical code.

## Where things live (oracle binary)

| Module (crate path) | Responsibility |
|---|---|
| `harness-core::config` | `harness.toml` load + cross-section validate |
| `harness-core::envelope` | JSON envelope contract every command emits |
| `harness-core::error` | typed `Error` + stable `ErrorCode` wire codes |
| `harness-core::path_guard` | safe write paths: `write_atomic` + `append_line` |
| `harness-core::sentinel` | the two reserved marker grammars harnex writes — managed regions + fill markers |
| `harness-core::markdown` | the one reader for what a rendered document shows — line splitting, fences, comments, ATX headings |
| `harness-core::evidence` | provenance verifier (strategy enum per claim shape) |
| `harness-core::telemetry` | append-only JSONL ledger with closed payload schema |
| `harness-core::codegen` | sentinel-block source → target sync |
| `harness-core::plan` | spec-workflow review grammar — finding rows, dispositions, decision-log convergence — computed by `plan audit` |
| `harness-core::policy` | permission rule grammar + profiles + version pins |
| `harness-core::routines` | scheduled harness tasks — closed frontmatter grammar + schedule states |
| `harness-core::scaffold` | composition manifest (`scaffold.toml`) + tier model |
| `harness-core::spec` | measurement stamps for the Claude Code vocabularies |
| `harness-core::validate` | rule / skill / agent / output-style / settings / commit-msg checks |
| `harness-core::audit` | harness-engineering compliance gate; `AuditCheckKind` is the check set |
| `harness-core::lifecycle` | observation + decision ledger + retirement |
| `harness-core::session` | reads Claude Code's own transcripts — instructions, interventions, repetition, tool and token use, and what the repository says survived |
| `harness-core::guard` | Claude Code hook adapter + Stop auditor + floor auditor (enforcement-surface freeze, hook-bypass tripwire) + telemetry emit (auto-records harness-element invocations) |
| `harness-core::governs` | rule `governs:` declarations — what a rule is truth about, resolved and audited |
| `harness-core::export` | JSON Schema emission |
| `harness-core::graph` | read-only `nodex` CLI bridge |
| `harness-core::check` | unified validation gate |
| `harness-cli` | thin clap wrapper; each command emits one envelope |

## Documentation map

- `plugins/harnex/` (`SKILL.md` + `reference/` + `templates/`) — the harnex
  plugin, distributed via `.claude-plugin/marketplace.json`; consumed by Claude
  Code when the plugin is installed, not by this repo's own sessions.
  `plugins/harnex/CLAUDE.md` is the editing contract, loaded when you work there.
- `README.md` — the only human-facing surface (the two surfaces, install,
  oracle quickstart, what the oracle covers).
- `.claude/rules/constitution.md` — always-loaded project laws.
- `.claude/rules/<topic>.md` — path-scoped guidance; loaded automatically
  when you read files matching that rule's `paths:` frontmatter.
- `crates/<crate>/CLAUDE.md` — crate-scoped guidance; loaded when you
  work inside that crate.
- `schemas/harness.schema.json` — JSON Schema for `harness.toml` (regen
  via `harnex export schema config --raw`).

For the full command surface, run `harnex --help` or read `README.md`.

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
