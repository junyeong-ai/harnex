---
name: harnex
description: Generate and maintain project-fit, project-native Claude Code harness tooling — hooks, settings.json, CLAUDE.md, path-scoped rules — in the target project's own language, from verified spec-correct templates. Use to set up a harness in a fresh repo, add a guardrail to an existing one, audit an existing harness for spec drift and over-constraint, or regenerate against the current Claude Code spec.
disable-model-invocation: true
argument-hint: "[scaffold|extend|audit|regenerate]"
---

# harnex

Engineer a Claude Code harness that fits THIS project, in ITS language. The
knowledge lives in `reference/`, the safety-critical pieces in `templates/`;
this skill composes them — it never free-generates a hook or a permission rule.

Read these first (they are the contract, load on demand):
- `${CLAUDE_PLUGIN_ROOT}/reference/spec-facts.md` — the Claude Code spec a
  generated harness MUST obey. Re-verify against the live docs each run.
- `${CLAUDE_PLUGIN_ROOT}/reference/enforced-vs-advisory.md` — where each
  guardrail belongs.
- `${CLAUDE_PLUGIN_ROOT}/reference/keep-soften-cut.md` — what never to impose.
- `${CLAUDE_PLUGIN_ROOT}/reference/language-matrix.md` — stack detection +
  per-language parameters.
- `${CLAUDE_PLUGIN_ROOT}/reference/exploration.md` — divide-and-conquer repo map.

Templates live under `${CLAUDE_PLUGIN_ROOT}/templates/{common,typescript,python}/`.
Generated files are written to `${CLAUDE_PROJECT_DIR}` (the target repo).

## Invariants (every mode)

1. **Compose templates; never free-generate safety-critical code.** Hook
   control flow, permission rules, and timeouts come from `templates/`. The LLM
   only selects the language profile and fills declared parameters.
2. **Enforced over advisory.** Must-happen → hook or `permissions.deny`.
   Guidance → short path-scoped rules. Workflow → a skill. (enforced-vs-advisory)
3. **Specific-but-minimal, never crude heuristics.** Apply keep-soften-cut:
   emit the KEEP set, ship SOFTEN as opt-in with an escape hatch, emit nothing
   from CUT. No natural-language pattern-matching in a blocking tier.
4. **Spec-correct.** Per spec-facts: hook `timeout` in seconds, Stop wrappers
   exit 0, `mcp__x__.*` matchers, `deny>ask>allow`, no project-ignored settings
   keys. When in doubt, re-read the live doc — freezing the spec is the failure.
5. **Right language.** Detect from lockfile+manifest; never cross-wire (biome
   for TS, ruff for Python). Never emit `node -e` / `python3 -c` into permissions.

## Mode: scaffold (greenfield)

A repo with no `.claude/`. 
1. Phase-1 fingerprint (exploration.md) → language profile. Single-package if no
   workspace globs — emit the lean variant (no per-module layer).
2. Compose into `${CLAUDE_PROJECT_DIR}`: `.claude/settings.json` (= common
   `permissions.deny.json` + `<lang>/permissions.allow.json` + common
   `hooks.json`), `hooks/` (`<lang>/_runner.sh`, common `_stop_runner.sh`,
   `<lang>/post-format.sh`, `<lang>/session-start.sh`, common `check-on-stop.sh`),
   `.claude/rules/constitution.md` (common), `CLAUDE.md` (common skeleton,
   filled with detected layout/commands).
3. Set hook scripts executable (0o755). Run `harness check` if the binary
   oracle is available.

## Mode: extend (brownfield, additive)

A repo that already has a harness. Run full exploration; read the incumbent
idiom from the module-map `existing_harness`.
1. Generate ONLY the requested additive artifact in the incumbent's idiom
   (its hook-runner pattern, its rule mechanism, its gate sequence).
2. Never overwrite `settings.json`, gate order, permissions, telemetry schema,
   or per-module CLAUDE.md. No dual SSoT (point codegen at the project's
   existing source via `source_format`, never a hand-maintained mirror).

## Mode: audit (read-only)

Explore; write nothing. Emit a gap report:
- Enforced-vs-advisory coverage (must-happen items living only in prose).
- keep-soften-cut violations (crude heuristics, CUT-tier noise, pedagogical
  "Why" essays in always-loaded files).
- Spec drift (millisecond `timeout`, brittle event-count assertions, a Stop
  hook that can exit non-zero, project-ignored settings keys).

## Mode: regenerate (spec drift)

Re-derive the generated artifacts against the CURRENT spec-facts (the case a
frozen binary cannot serve). Diff against what exists; rewrite only the drifted
files; preserve project-authored content. Report what changed and why.

## Verify before finishing

Generated shell hooks pass `bash -n`; generated JSON parses; the harness the
skill emits would itself pass `harness check` / `harness validate settings`.
For UI-less generation, state what was emitted and what the operator must run.
