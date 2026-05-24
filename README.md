# harnex

Harness engineering for Claude Code projects. Pure-Rust, JSON-first
CLI that enforces deterministic harness invariants — provenance verification,
closed-schema telemetry, lifecycle promotion / retirement, runtime guards,
unified validation gate — without taxing modern models with obsolete
prescriptive checklists.

## Why

Modern Claude follows in-context conventions well. What it cannot do alone
is maintain cross-session state, enforce multi-operator agreement, or
deterministically reject what the runtime would silently corrupt. This
toolkit covers exactly that gap.

## Install

```bash
cargo build --release          # → ./target/release/harness
```

Requires Rust 1.95+.

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
harness export schema config --raw > schemas/harness.schema.json
```

(`--raw` emits the bare schema; without it the schema is wrapped in the
standard JSON envelope for programmatic consumers.)

## Quickstart (new project)

```bash
cd your-project/

# 1. Scaffold harness.toml, CLAUDE.md, README.md, .claude/{rules,settings.json}, hooks/
./harness init --name my-project              # includes hook scripts
./harness init --name my-project --no-hooks   # skip hook generation

# 2. Run the unified gate — every enabled validator in one JSON envelope
./harness check

# 3. Auto-fix what can be fixed (currently: codegen sync)
./harness check --fix

# 4. Generate baseline shell-completion (optional)
./harness completions zsh --raw > ~/.zsh/completions/_harness
```

The generated `harness.toml` enables: evidence (provenance verifier),
telemetry (event ledger), validate.rules/skills, policy.permissions
(baseline deny), lifecycle (promotion/retirement). Extend with
`[[kinds]]`, `[[lifecycle.consumer_detectors]]`, `[[codegen.groups]]`,
`[[policy.versions]]`, `[validate.commit_msg]` as your project grows.

## Command surface (12 groups)

```
harness check [--since <ref>] [--fix]                  # unified validation gate
harness init [--name N] [--dir D] [--force] [--dry-run] [--no-hooks]

harness evidence verify <files...>
harness telemetry append --kind K --payload <json>
harness telemetry count --kind K [--since <rfc3339>]
harness telemetry report [--kind K] [--window 1,7,30,90]

harness codegen sync | check

harness policy permissions generate | audit [--path <p>]
harness policy versions show | check --tool T --installed V

harness validate rules <files...>
harness validate skills <files...>
harness validate settings [<path>]
harness validate commit-msg <path>                     # closed-enum trailer

harness lifecycle observe --tag T --text X --source S
harness lifecycle candidates
harness lifecycle promote --tag T --text X --decision-text "..."
harness lifecycle reject  --tag T --text X --decision-text "..."
harness lifecycle defer   --tag T --text X --decision-text "..."
harness lifecycle demote  --tag T --text X --decision-text "..."
harness lifecycle classify --kind K --path P [--silent]
harness lifecycle retire [--window N]
harness lifecycle decisions [--tag T] [--decision D]

harness guard hook-event                               # parse stdin hook JSON
harness guard hook-run <prog> [args...]                # standard hook wrapper
harness guard hook-stop <prog> [args...]               # Stop hook (always exit 0)
harness guard stop-audit [--session ID]                # fresh-context Stop audit

harness graph version | backlinks <id> | orphans | stale | nodes --kind K | diff <a> <b>

harness export schema {config|envelope|finding|event|permissions|error-codes|all}

harness completions <bash|zsh|fish|powershell|elvish> [--raw]
```

Every command emits one JSON envelope on stdout. Exit code: 0 = success,
1 = blocking finding, 2 = runtime failure.

## Porting existing projects

The toolkit replaces the universal Claude Code harness patterns. Project-
specific lint (language ASTs, design tokens, package allowlists, multi-phase
spec workflows) correctly stays in the project.

### Webloom (TypeScript monorepo) — ~72% coverage

| webloom | harness equivalent |
|---|---|
| `tools/closed-enums-build` | `harness codegen sync` |
| `tools/feedback-promote` + `tools/pattern-promote` | `harness lifecycle observe/candidates/promote/reject/defer/demote` |
| `tools/retirement-audit` | `harness lifecycle retire` |
| `tools/version-check` | `harness policy versions` |
| `tools/skill-eval` | `harness validate skills` |
| `tools/telemetry` | `harness telemetry` |
| `tools/harness-lint` | `harness validate settings` |
| `tools/drift-lint` | `harness evidence verify` (partial) |
| `tools/context-audit` | `harness telemetry` + `InstructionsLoaded` hook (integration) |
| `tools/envelope` / `tools/frontmatter` | `harness-core::envelope` / `validate::frontmatter` |
| `.claude/skills/webloom-demote` | `harness lifecycle demote` |
| `.claude/skills/webloom-eval` | `harness validate skills` |
| `hooks/_runner.sh` | `harness guard hook-run` |
| `hooks/_stop_runner.sh` | `harness guard hook-stop` |
| `hooks/commit-msg` | `harness validate commit-msg` |
| `pre-commit` / `pre-push` orchestration | `harness check` |
| `architecture-lint`, `portability-check`, `resources-lint`, `a11y-audit`, `design-*`, `nodex-types-build` | **stays in webloom** (TS / GCP / DTCG domain-specific) |
| `webloom-spec`, `webloom-review`, `webloom-critique`, etc. | **stays in webloom** (project workflow) |

### AIX Platform (Python monorepo) — ~45% coverage

| aix | harness equivalent |
|---|---|
| `scripts/audit_loop.py` + `_auditor.py` + `check_on_stop.py` | `harness guard stop-audit` |
| `scripts/aix_versions.py` | `harness policy versions` |
| `scripts/lint_enum_sync.py` | `harness codegen check` |
| `scripts/hooks/check_commit_msg.py` | `harness validate commit-msg` |
| `scripts/hooks/skill_telemetry.py` / `mcp_telemetry.py` | `harness telemetry append` |
| `scripts/hooks/_runner.sh` / `_stop_runner.sh` | `harness guard hook-run` / `hook-stop` |
| `lint_pr_body.py` | (roadmap: `harness validate pr-body`) |
| `lint_dep_licenses.py` | (roadmap: `harness policy licenses`) |
| `lint_{agent_module,alembic_orm_parity,artifact_drift,conventions,deprecated_annotations,dockerfile_workspace_sync,harness_telemetry_pii,observability,opentofu_env,pipeline_handler_parity}` | **stays in aix** (Python AST, SQLAlchemy, Docker, OpenTofu) |
| `aix-{spec,review,critique,debug}` | **stays in aix** (project workflow) |

### Generic projects (any language)

Universal patterns covered out-of-the-box:
- Provenance verification on docs
- Append-only telemetry with closed payload schema
- Sentinel-block enum codegen across N files
- Permission profiles (7 built-in: baseline, git-strict, gcp-strict, aws-strict, rust-dev, python-dev, typescript-dev) for Claude Code settings
- Claude Code spec compliance (rules / skills / settings frontmatter)
- Promotion + retirement lifecycle for learnings
- Settings.json hook adapter (29 documented events)
- Single-command CI gate

Project-specific lint (language AST, internal data model, design system,
multi-phase spec orchestrators) is intentionally out of scope — those
belong with the project's domain knowledge, not in a generic toolkit.

## Operating context

Day-to-day operation is delegated to Claude Code. See `CLAUDE.md` and
`.claude/rules/` for the AI operating context. This README is the only
file written for humans; everything else under this repo is consumed
directly by Claude.

## License

MIT.
