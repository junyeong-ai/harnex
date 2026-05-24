# Exploration (divide-and-conquer)

How harnex builds a map of a repo before generating or auditing a harness.
The brownfield modes (`extend`, `audit`) run all three phases; `scaffold` runs
only Phase 1 to pick the language profile.

Principle: read manifests first, never the whole repo. Every frontier model
degrades as irrelevant context accumulates (context rot), well below the
window limit — so exploration is a graph traversal, not a sweep.

## Phase 1 — deterministic fingerprint (no LLM judgment)

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

Output: the repo-level facts of the module-map artifact (below). If no
workspace globs → single-package repo; skip Phase 2, generate the lean profile.

## Phase 2 — module exploration (fan out ONLY when it pays)

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

Aggregate by having each subagent write its entry to the **module-map
artifact**, not by funneling transcripts back through the orchestrator —
detailed results flooding the lead is the aggregation-loss / context-blowup
pitfall.

## Module-map artifact (the structured aggregation point)

A single JSON file the synthesis step reads (not the transcripts):

```json
{
  "repo": {
    "monorepo": true,
    "package_manager": "pnpm",
    "workspace_tool": "turborepo",
    "language_primary": "typescript",
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

## Phase 3 — synthesis (single agent)

Read the artifact (never the raw exploration transcripts). Decide the harness
plan: which language profile, which template files to emit, and — in brownfield
— which incumbent artifacts to respect and never overwrite. Generation is
single-agent and sequential; code changes are stateful, so do not parallelize
them.

## Brownfield respect (extend / audit)

When `existing_harness` is present, match the incumbent idiom (its hook-runner
pattern, its rule mechanism, its gate sequence). Produce additive artifacts
only; never regenerate `settings.json`, the gate order, permissions, telemetry
schema, or per-module CLAUDE.md. `audit` writes nothing — it emits a gap report
(enforced-vs-advisory coverage, keep-soften-cut violations, spec drift such as
a millisecond `timeout`).
