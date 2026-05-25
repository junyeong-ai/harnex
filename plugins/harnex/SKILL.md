---
name: harnex
description: Generate and maintain project-fit, project-native Claude Code harness tooling — hooks, settings.json, CLAUDE.md, path-scoped rules — in the target project's own language, from verified spec-correct templates. Use to scaffold a harness in a fresh repo, extend one with a closed-verb additive change, audit an existing harness for spec drift, or regenerate the managed regions against the current Claude Code spec.
disable-model-invocation: true
argument-hint: "scaffold | extend <verb> <args> | audit | regenerate"
---

# harnex

Engineer a Claude Code harness that fits THIS project, in ITS language. The
knowledge lives in `reference/`, the safety-critical pieces in `templates/`;
this skill composes them — it never free-generates a hook or a permission rule.

Read these first (they are the contract, load on demand):
- `${CLAUDE_SKILL_DIR}/reference/spec-facts.md` — the Claude Code spec a
  generated harness MUST obey. Re-verify against the live docs each run.
- `${CLAUDE_SKILL_DIR}/reference/enforced-vs-advisory.md` — where each
  guardrail belongs.
- `${CLAUDE_SKILL_DIR}/reference/keep-soften-cut.md` — what never to impose.
- `${CLAUDE_SKILL_DIR}/reference/language-matrix.md` — stack detection +
  per-language parameters.
- `${CLAUDE_SKILL_DIR}/reference/exploration.md` — divide-and-conquer repo map.

Templates live under `${CLAUDE_SKILL_DIR}/templates/`: language-agnostic
pieces in `common/`, and one directory per supported language
(`typescript/`, `python/`, `rust/` today — adding a language is a new
`<lang>/` directory plus its `*-dev` permission profile). Generated files are
written to `${CLAUDE_PROJECT_DIR}` (the target repo).

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
   exit 0, hook MCP matchers `mcp__server__tool` or `mcp__server__.*` (bare
   `mcp__server` matches NOTHING in a hook matcher — that bare form is
   permission-rule syntax only), `Bash(cmd *)` space-wildcards,
   `deny>ask>allow`, no project-ignored settings keys. When in doubt, re-read
   the live doc — freezing the spec is the failure.
5. **Right language.** Detect from lockfile+manifest; never cross-wire (biome
   for TS, ruff for Python, rustfmt for Rust). Never emit `node -e` /
   `python3 -c` into permissions. Never grant built-in read-only commands
   (`ls`, `grep`, `cat`, read-only `git`) — they never prompt, so an allow is a
   no-op; grant only commands that actually prompt.
6. **Managed-region contract.** A generated artifact is partitioned into
   harnex-managed regions (delimited by
   `<!-- harnex-managed:start <slug> --> ... <!-- harnex-managed:end <slug> -->`)
   and project-authored regions (everything else). `regenerate` only touches
   the managed regions; `extend` only adds new regions in the incumbent
   idiom; an audit flags edits inside managed regions for operator review.
   For `.claude/settings.json` (JSON, no comments), the partition is by
   top-level key: `permissions`, `hooks` are harnex-managed; every other
   top-level key is project-owned.

## Mode: scaffold (greenfield)

A repo with no `.claude/`.

### Step 1 — Deep project analysis

Run the full Phase-1 fingerprint (exploration.md), PLUS the following
project-specific analysis. The goal: every generated file is pre-filled
with project-fit content, not blank placeholders.

| Analyze | Source | Feeds into |
|---|---|---|
| Language + package manager | lockfile + manifest | template selection |
| Monorepo structure | workspace config | lean vs multi-package scaffold |
| Build / test / lint commands | Makefile, Justfile, package.json `scripts`, pyproject.toml `[tool.just]`/`[project.scripts]`, CI config | CLAUDE.md `## Build & test` |
| Directory layout | top-level `ls` + workspace member dirs | CLAUDE.md `## Layout` |
| Project description | README.md first paragraph, manifest `description` field | CLAUDE.md header |
| Formatter / linter / type checker | biome.json, .eslintrc, ruff in pyproject.toml, rustfmt.toml, tsconfig.json | CLAUDE.md `## Conventions`, post-format hook config |
| Existing CI pipeline | `.github/workflows/*.yml`, `.gitlab-ci.yml`, `Jenkinsfile` | hook event selection, gate sequence |
| Existing test framework | vitest.config, pytest.ini, Cargo test | `<lang>-conventions.md` testing section |
| Security tooling | gitleaks, semgrep, CodeQL, `npm/pip/cargo audit`, IaC scanners (in deps or CI) | suggest `gcp-strict`/`aws-strict` profile; secret-scan recommendation |

For a monorepo, analyze per workspace member when packages differ in
toolchain or test framework (exploration Phase 3) — a single root profile
flattens real per-package differences.

### Step 2 — Compose artifacts from templates + analysis

- `.claude/settings.json` (`permissions` = common `permissions.deny.json` +
  `<lang>/permissions.allow.json`; `hooks` = common `hooks.json`).
  If CI config reveals additional tools the project uses (docker, terraform,
  gcloud), suggest composing with `gcp-strict` or `aws-strict` profiles.
- `hooks/` — Claude Code hook scripts (`<lang>/_runner.sh`, common
  `_stop_runner.sh`, `<lang>/post-format.sh`, common `session-start.sh`,
  common `check-on-stop.sh`) AND the git pre-commit hook (common
  `git-hooks/pre-commit` → `hooks/pre-commit`, runs gitleaks). The two
  hook kinds coexist: git runs only files named after git events
  (`pre-commit`), Claude Code runs the `_runner.sh`-dispatched scripts.
- `.claude/rules/constitution.md` (common, managed region wraps the
  articles — the path-scoped rules added later sit beside it untouched).
- `.claude/rules/governance.md` (common — self-improvement gatekeeper:
  observation sink, promotion gate split advisory rule vs enforced guardrail,
  and the oracle loop commands for surfacing candidates).
- `.claude/rules/artifact-lifecycle.md` (common — promotion path from
  observation → validated pattern → rule; retirement criteria).
- `CLAUDE.md` — **LLM fills from analysis, not blank placeholders**:
  - `# <project-name>` — from manifest `name` or README title.
  - `## Layout` — from directory scan. One line per top-level area;
    let the agent read manifests for detail rather than enumerating
    every file. Include workspace member directories if monorepo.
  - `## Build & test` — exact commands from Makefile/Justfile/package.json
    scripts. Format: `<command>` — `<what it does>`.
  - `## Conventions` — only decisions the formatter doesn't enforce.
    State the formatter/linter/type-checker in use (observed from config)
    and any project-specific patterns found in the codebase.
  - `## Enforcement` — harnex-managed region (from template).
- Optionally one `<lang>/rules/<lang>-conventions.md` as a starting
  path-scoped rule — customize to the detected test framework, toolchain.

### Step 3 — Finalize

Set hook scripts executable (0o755), including `hooks/pre-commit`. Point git
at the version-controlled hooks: `git config core.hooksPath hooks` (state
this command for the operator to run; do not run git config silently).
Verify: `bash -n` on every `.sh` and on `hooks/pre-commit`, JSON-parse
settings.json. Run `harness check` / `harness audit` if the binary oracle is
available. Report what was generated and suggest `extend pattern` additions
based on what the analysis revealed (e.g., CI deploy stages →
`extend pattern spec-workflow`).

## Mode: extend (brownfield, additive — closed verb menu)

Free-form additive generation invites free-form free-generation. The verb
menu below enumerates the closed set; refuse any other request and ask the
operator to re-phrase using a verb from this list.

- **`extend hook <event-name>`** — add a hook for `<event-name>` (must be in
  spec-facts hook events). The runner selection is safety-critical and
  template-driven: `Stop` and `SubagentStop` dispatch through
  `_stop_runner.sh` (forces exit 0 — for these events exit 2 specifically
  prevents the stop and forces continuation, the re-stop loop; other non-zero
  codes are non-blocking errors). Every other event — including `StopFailure`,
  whose exit 2 is genuinely ignored — dispatches through `_runner.sh`
  (propagates exit code). The verifier script's BODY is
  project-specific check logic the operator authors — that is not free-
  generated safety-critical control flow, which lives entirely in the two
  runner templates. Add the event entry to `.claude/settings.json` `hooks`
  (the managed region) with the correct runner per the rule above; for a
  PreToolUse/PermissionRequest matcher targeting MCP, use
  `mcp__server__tool` / `mcp__server__.*`, never bare `mcp__server`.
- **`extend rule <slug> <paths-glob>`** — drop a path-scoped rule at
  `.claude/rules/<slug>.md` with the given `paths:` frontmatter. Body is a
  short imperatives skeleton (heading + 3-5 bullets) — the operator fills.
- **`extend permission deny <pattern>`** — append `<pattern>` to the
  `permissions.deny` array in `.claude/settings.json`. The pattern must
  follow the spec grammar (`Bash(cmd *)`, `Read(path)`, `Edit(path)`,
  `Write(path)`, `WebFetch(domain:...)`, `mcp__server[__tool]`).
- **`extend permission ask <pattern>`** — same, into `permissions.ask`.
- **`extend permission allow <pattern>`** — same, into `permissions.allow`.
  Refuse when `<pattern>` is a read-only built-in (`ls`, `grep`, `cat`,
  read-only `git`) — its allow rule is a no-op.
- **`extend language <lang>`** — bootstrap a new language directory:
  `templates/<lang>/{_runner.sh, post-format.sh, permissions.allow.json}`
  + the matching `<lang>-dev` profile stub in `profiles.rs`
  (`session-start.sh` is common, not per-language). Operator fills the
  toolchain commands; the `policy_template_sync` reverse-gap test enforces
  both sides exist.
- **`extend pattern <name>`** — install a proven engineering pattern,
  **customized to the target project**. The pattern set and each pattern's
  files + analysis steps are declared in
  `${CLAUDE_SKILL_DIR}/templates/patterns/manifest.toml` (the SSoT; a drift
  test keeps it in sync with the directories). Flow:
  1. Read the manifest entry + skeleton from `templates/patterns/<name>/`.
  2. Explore the project (Phase-1 fingerprint + the entry's `analyze` steps).
  3. Customize the skeleton's defaults based on what you observe.
  4. Write each `files` entry's `template` to its declared `destination`
     under `${CLAUDE_PROJECT_DIR}` (the manifest owns destinations).
  The template provides proven structure + defaults; the LLM replaces
  generic defaults with project-specific observations. Every `<!-- Fill in
  -->` / `<!-- Customize -->` marker MUST be replaced — with an observed
  value, or an explicit "none observed yet — <default behavior>" note.
  Never leave a raw fill-in marker in a generated file; a placeholder that
  ships is the blank-page problem in disguise.

  **Per-pattern analysis instructions:**
  - `naming-decisions` — scan file names (dominant casing), imports
    (factory verb patterns), type definitions (parameter bag suffixes),
    tool scripts (suffix conventions). Pre-fill each section with observed
    patterns. Flag `## Domain vocabulary` for operator input.
  - `copy-conventions` — detect locale from string literals. Detect error
    message format from existing error handling code. Detect i18n framework
    from dependencies (next-intl, react-i18n, gettext, fluent). Pre-fill
    register and terminology with observations.
  - `review-lenses` — auto-link lens `anchors:` to the project's existing
    `.claude/rules/` files. Customize each lens's `applies_to:` based on what
    file types the project has.
  - `spec-workflow` — check for existing `specs/` or `docs/adr/` directory.
    If found, adapt template structure to match existing layout instead of
    overwriting. Map CI stages to gates if CI config exists.
  - `observability` — detect logging/tracing framework (structlog, winston,
    tracing, OpenTelemetry SDK). Pre-fill namespace prefix from the project
    name. Adapt span naming examples to the detected framework.
  - `deprecation` — detect existing deprecation markers (`@deprecated`
    decorators, JSDoc tags, `#[deprecated]` attributes). Adapt the
    allow-marker format to complement, not conflict with, the language's
    native deprecation mechanism.
  - `pr-conventions` — check for existing `.github/pull_request_template.md`.
    If found, merge harnex defaults into the existing template's structure
    rather than replacing it.

  Available patterns:
  - `review-lenses` — convergent review loop + 6 default lens files.
  - `spec-workflow` — 5-phase spec pipeline (specify → plan → implement →
    validate → wrapup) + optional preview/deploy.
  - `observability` — span naming, PII boundary, baseline-before-alert.
  - `deprecation` — allow-marker grammar with sunset dates.
  - `pr-conventions` — PR template + AI-fill discipline.
  - `naming-decisions` — team naming vocabulary (tool suffixes, factory
    verbs, parameter bags, domain terms).
  - `copy-conventions` — communication register, terminology, error
    message format, i18n.

In every verb: read the module-map's `existing_harness` first; match the
incumbent hook-runner pattern, rule mechanism, and gate sequence. Never
overwrite `settings.json` top-level keys outside the verb's scope.

## Mode: audit (read-only, envelope output)

Drive `harness audit` and present its `AuditOutcome` envelope to the
operator. Findings:
- `audit-ms-timeout` — hook timeout values that look like milliseconds
  (≥ 1000) instead of seconds.
- `audit-mcp-matcher-incomplete` — `mcp__server` matcher without the
  required `__.*` suffix (matches nothing).
- `audit-stop-blocking-suspect` — `Stop` hook whose script can plausibly
  exit non-zero (no explicit `exit 0`).
- `audit-managed-region-edited` — content inside a `harnex-managed`
  region diverges from the corresponding template.

The CLI emits an envelope; no skill-side prose synthesis required.

## Mode: regenerate (spec drift)

Re-derive the managed regions against the CURRENT spec-facts (the case a
frozen binary cannot serve). For each file with sentinel markers:
1. Read the existing file. Extract project-authored regions (everything
   outside managed sentinels).
2. Render the managed regions fresh from the current template + language
   profile.
3. Write the file back with project-authored regions preserved verbatim.

For `.claude/settings.json`: rewrite only the top-level `permissions` and
`hooks` keys; preserve every other key (`autoMemoryEnabled`,
`skillOverrides`, `env`, etc.).

Report what changed and why.

## Verify before finishing

Generated shell hooks pass `bash -n`; generated JSON parses; the harness the
skill emits would itself pass `harness check` / `harness validate settings`
/ `harness audit`. For UI-less generation, state what was emitted and what
the operator must run.
