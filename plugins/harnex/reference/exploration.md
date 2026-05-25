# Exploration (divide-and-conquer)

How harnex builds a map of a repo before generating or auditing a harness.
Two orthogonal axes: **what concern areas to analyze** (phases 1–2) and
**how to scale that analysis across a monorepo** (phase 3 fan-out).

| Mode | Phase 1 | Phase 2 | Phase 3 | Phase 4 |
|---|---|---|---|---|
| `scaffold` | ✓ fingerprint | ✓ project profile | if ≳4 modules | ✓ synthesis |
| `extend` | ✓ | ✓ (the touched area) | rarely | ✓ |
| `audit` | ✓ | — | if ≳4 modules | ✓ |
| `regenerate` | ✓ | — | — | ✓ |

Principle: read manifests first, never the whole repo. Every frontier model
degrades as irrelevant context accumulates (context rot), well below the
window limit — so exploration is a graph traversal, not a sweep.

## Phase 1 — structural fingerprint (deterministic, no LLM judgment)

Read only these, in order, and extract structural facts:

1. **Lockfile + manifest** → language + package manager (per language-matrix):
   `pnpm-lock.yaml`+`package.json` ⇒ TS/pnpm · `uv.lock`+`pyproject.toml` ⇒
   Python/uv · `Cargo.lock`+`Cargo.toml` ⇒ Rust/cargo.
2. **Workspace config** → monorepo? module globs: `pnpm-workspace.yaml`,
   `[tool.uv.workspace].members`, `[workspace].members`, `turbo.json`.
3. **Toolchain config** → formatter / typecheck / gate runner: `biome.json`,
   `ruff`/`ty` in `pyproject.toml`, `Justfile`, `.pre-commit-config.yaml`.
4. **Existing harness** (brownfield signal) → `.claude/settings.json`
   (hooks + permissions), `.claude/rules/`, `.claude/skills/`, `hooks/`,
   `CLAUDE.md` (root + count), `autoMemoryEnabled`.

Output: the `repo` facts of the module-map artifact. If no workspace globs →
single-package repo; skip the Phase-3 fan-out, generate the lean profile.

## Phase 2 — project profile (concern-area analysis for generated content)

Phase 1 picks the template; Phase 2 fills it. Each concern area is read from
its canonical source so generated artifacts are project-fit, not blank
placeholders. Read the source, not the whole tree.

| Concern | Source | Feeds |
|---|---|---|
| Build / test / lint commands | Makefile, Justfile, `package.json` scripts, `[project.scripts]`, CI config | CLAUDE.md `## Build & test`; gate sequence |
| Directory layout | top-level listing + workspace member dirs | CLAUDE.md `## Layout` |
| Project description | README first paragraph, manifest `description` | CLAUDE.md header |
| Code conventions | formatter/linter/type-checker config + a sample of source | CLAUDE.md `## Conventions`; `<lang>-conventions.md` |
| CI pipeline + gates | `.github/workflows/*.yml`, `.gitlab-ci.yml`, `turbo.json` | hook event selection; suggested `extend pattern` |
| Test framework | `vitest.config`, `pytest.ini`, Cargo test layout | `<lang>-conventions.md` testing section |
| Security tooling | gitleaks, semgrep, CodeQL, `npm/pip/cargo audit`, IaC scanners in deps/CI | suggested `gcp-strict`/`aws-strict` profile; secret-scan recommendation |

A concern with no signal keeps its template default and is noted "none
observed yet" — never a guessed value. `extend` runs only the rows its verb
touches (e.g., `extend pattern naming-decisions` reads file-name casing +
import verbs).

For a monorepo, run Phase 2 per workspace member when packages differ in
toolchain or test framework — a single root profile flattens real
per-package differences. The Phase-3 fan-out (below) supplies the
per-module facts; synthesis writes per-package `<lang>-conventions.md` only
where a package genuinely diverges from the repo default.

## Phase 3 — module exploration (fan out ONLY when it pays)

Enumerate modules from the workspace globs. Fan out a read-only Explore
subagent per module **only when** modules are independent (clean boundaries,
no bidirectional deps) and there are enough to matter (≳4). Otherwise read the
module roots directly with a single agent.

Never fan out for: small/single-package repos, dependency-heavy or stateful
work, or the generation itself. Fan-out costs 4–15× tokens and hurts on
dependent tasks; it is for read-heavy exploration only. Subagents cannot spawn
subagents.

Each Explore subagent gets an explicit objective, output format, and scope
boundary — vague delegation causes overlap and gaps. Explore agents are
read-only and skip CLAUDE.md/git-status by design, keeping their context small;
they return only a summary.

Per-module delegation template:
> Explore `<module-path>` only. Report as the module-map `modules[]` entry
> (path, kind, language, toolchain, depends_on, has_harness). Read its manifest
> and entrypoints; do not read sibling modules; do not propose changes.

Aggregate by having each subagent RETURN its compact `modules[]` entry (the
fields above, nothing more) and the orchestrator append it to the **module-map
artifact**. Explore agents are read-only, so they cannot write the artifact —
returning only the structured entry (never the full transcript) is also what
keeps the lead from drowning in detail: the aggregation-loss / context-blowup
pitfall is funneling transcripts back, not returning one small row each.

## Module-map artifact (the structured aggregation point)

A single JSON file the synthesis step reads (not the transcripts):

```json
{
  "repo": {
    "monorepo": true,
    "package_manager": "pnpm",
    "workspace_tool": "turborepo",
    "language_primary": "typescript",
    "profile": {
      "build_commands": ["pnpm build", "pnpm test", "pnpm type-check"],
      "formatter": "biome", "test_framework": "vitest",
      "ci_gates": ["lint", "test", "build"], "description": "…"
    },
    "existing_harness": {
      "settings": true, "hooks": ["SessionStart","PostToolUse","Stop"],
      "rules_count": 21, "claude_md_count": 16,
      "hook_runner_idiom": "bash hooks/_runner.sh <verifier>",
      "pretooluse_blocking": false, "auto_memory_enabled": false
    }
  },
  "modules": [
    {
      "path": "apps/web", "kind": "app", "language": "typescript",
      "toolchain": { "formatter": "biome", "typecheck": "tsc", "gate": "pnpm" },
      "depends_on": ["packages/ui"], "has_harness": false
    }
  ]
}
```

## Phase 4 — synthesis (single agent)

Read the artifact (never the raw exploration transcripts). Decide the harness
plan: which language profile, which template files to emit, the project-fit
content for each from the `profile` block, and — in brownfield — which
incumbent artifacts to respect and never overwrite. Generation is single-agent
and sequential; code changes are stateful, so do not parallelize them.

## Brownfield respect (extend / audit)

When `existing_harness` is present, match the incumbent idiom (its hook-runner
pattern, its rule mechanism, its gate sequence). Produce additive artifacts
only; never regenerate `settings.json`, the gate order, permissions, telemetry
schema, or per-module CLAUDE.md. `audit` writes nothing — it emits a gap report
(enforced-vs-advisory coverage, keep-soften-cut violations, spec drift such as
a millisecond `timeout`).
