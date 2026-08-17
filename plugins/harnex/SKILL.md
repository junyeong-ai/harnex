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
(`typescript/`, `python/`, `rust/`, `jvm/` today — adding a language is a new
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
4. **Spec-correct.** Every generated artifact obeys spec-facts. Specifics:
   - Hook `timeout` is in **seconds** (never milliseconds).
   - Stop/SubagentStop wrappers **exit 0** (exit 2 forces continuation).
   - Hook MCP matchers: `mcp__server__tool` or `mcp__server__.*` — bare
     `mcp__server` matches NOTHING (that form is permission-rule syntax only).
   - Permission wildcards: `Bash(cmd *)` with space-then-`*`.
   - Evaluation order: `deny > ask > allow`, first-match-wins.
   - Never emit project-scope no-op keys into `.claude/settings.json`.
   When in doubt, re-read the live doc — freezing the spec is the failure.
5. **Every language present, and a floor without one.** Detect from
   lockfile+manifest and match *every* row whose signal is there — the answer
   is a set, and the language tier is emitted once per member. Never
   cross-wire (biome for TS, ruff for Python, rustfmt for Rust), and never
   resolve two present stacks by row order: that is the wrong-profile failure
   with extra steps. When no supported stack matches, emit the manifest's
   **foundation tier** — the
   permission floor, the foundation rules, the hook wrappers, the secret-scan
   git hook, all language-agnostic — and report exactly which language-tier
   artifacts are unavailable and why. What is forbidden is a *wrong* profile,
   not an absent one: guessing a formatter is the meta-failure, while
   withholding a floor the stack never needed a profile for helps nobody.
   Never emit `node -e` / `python3 -c` into permissions. Never grant built-in
   read-only commands (`ls`, `grep`, `cat`, read-only `git`) — they never
   prompt, so an allow is a no-op; grant only commands that actually prompt.
6. **Managed-region contract.** A generated artifact is partitioned into
   harnex-managed regions (delimited by
   `<!-- harnex-managed:start <slug> --> ... <!-- harnex-managed:end <slug> -->`)
   and project-authored regions (everything else). `regenerate` only touches
   the managed regions; `extend` only adds new regions in the incumbent
   idiom; an audit flags edits inside managed regions for operator review.
   For `.claude/settings.json` (JSON, no comments), ownership is **item-level
   within** `permissions` and `hooks`, NOT whole-key: harnex owns only the
   entries it generated — the baseline + `<lang>-dev` permission rules and the
   base hook entries (SessionStart `startup|resume`, PostToolUse `Edit|Write`,
   Stop), each identified by its template shape (event + matcher + runner
   script). Entries an operator added via `extend`, and any incumbent
   hand-rolled entries, are project-owned and survive regenerate. Every other
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
| Build / test / lint commands **and the runner that drives them** | Makefile, Justfile, `package.json` `scripts`, pyproject.toml `[tool.poe.tasks]`/`[tool.hatch.envs.*.scripts]`/`[tool.pdm.scripts]`/`[project.scripts]`, Taskfile.yml, mise `[tasks]`, CI config | CLAUDE.md `## Build & test` **and** the gate-driver grant (language-matrix) |
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

**The file set is `${CLAUDE_SKILL_DIR}/templates/scaffold.toml`, not this
list.** Read it and emit every artifact it declares, dispatching on
`content.kind` — into a destination that does not exist, `copy`, `seed` and
`managed` are written verbatim and `merge` contributes its JSON fragment at
`content.key` as a union where two artifacts name the same key (the manifest
header states the rule). `chmod 0o755` where `executable` is set. Emit the
`foundation` tier always, and the `language` tier once per detected stack,
resolving `{lang}` to that language each time. The manifest is the single home
for that set — a second list here would be the one that drifts, and the
oracle's fixture test builds from the same file so a scaffold and its guard
cannot disagree.

**Check every destination before writing it.** A repo with no `.claude/` — the
repo scaffold mode is for — usually still has a `CLAUDE.md`, because Claude
Code reads one without any `.claude/` directory, and a repo with git hooks
already has `hooks/`. The manifest header states the rule per kind: `copy` and
`seed` keep the incumbent and emit nothing; `managed` contributes only its
sentinel blocks and leaves every other byte alone; `merge` unions. Never
replace a file the project wrote. Report every collision you left in place,
naming what harnex would have put there, so the operator can merge by hand.

`content.kind` also says what happens to each artifact afterwards, which is
what the operator needs to hear when you report: `copy` is machinery to leave
alone, `seed` is theirs to edit from the first commit, `managed` is theirs
outside the sentinels, `merge` shares its destination with the other tier and
with their own entries.

One artifact is a skill rather than a rule, and the rubric is why: the
promotion-and-retirement pass `governance.md` describes is a procedure over
several commands with a decision at each step, and invariant 2 sends a workflow
to a skill. It also needs `allowed-tools`, which a rule cannot carry, and it
must be in context when someone runs the sweep rather than when they happen to
edit a rule — which is all a `paths:` scope can offer. Every other foundation
artifact stays a rule because it is guidance, not a procedure.

The manifest declares template-derived artifacts. Three emissions are outside
it because their content comes from the project rather than from a template:
- For Rust, `rustfmt.toml` carrying the edition declared in `Cargo.toml` —
  per-file `rustfmt` does not read the manifest and would otherwise format to
  a different style than `cargo fmt` (language-matrix).
- Composing `gcp-strict` or `aws-strict` into `permissions` when CI config
  reveals the project uses docker / terraform / gcloud.
- The gate-driver grant, from the task declaration Step 1 already read (the
  language-matrix fingerprint table). A project whose gates run through `just`,
  `make`, `poe` or `task` prompts on every gate invocation without it, and the
  language toolchain grant does not cover the runner that wraps it. No signal,
  no grant — an undeclared runner is the same guess as the wrong formatter.

Git hooks and Claude Code hooks coexist in `hooks/`: git runs only files named
after git events (`pre-commit`), Claude Code runs the `_runner.sh`-dispatched
scripts.

Content the manifest cannot supply, because it comes from Step 1's analysis:

- `CLAUDE.md` — **LLM fills from analysis, not blank placeholders.** Use a
  fixed source precedence so two operators on the same repo converge:
  - `# <project-name>` — manifest `name` first; README title only if the
    manifest has none.
  - description line — manifest `description` first; else README's first
    paragraph.
  - `## Layout` — from directory scan. One line per top-level area;
    let the agent read manifests for detail rather than enumerating
    every file. Include workspace member directories if monorepo.
  - `## Build & test` — exact commands from the canonical task source, in this
    order: Makefile/Justfile > `package.json` scripts / `[project.scripts]` >
    CI config. List the project's declared gate sequence in its declared order
    (do not reinvent ordering). Format: `<command>` — `<what it does>`.
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
available. On a repo that already had artifacts, the scaffolded `harness.toml`
points validators at them for the first time, so report those findings as part
of what the scaffold revealed rather than leaving them to be discovered — a
brownfield harness typically has some, and they are the reason the scaffold was
worth running. Report what was generated and suggest `extend pattern` additions
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
- **`extend skill <name>`** — scaffold a spec-correct domain skill at
  `.claude/skills/<name>/SKILL.md` from `common/skill-template.md`. Frontmatter
  is composed correct-by-spec (description+when_to_use ≤ 1536 chars, body
  ≤ 500 lines, `disable-model-invocation: true` so an unfinished or
  side-effecting skill never auto-fires); the operator fills the procedure and,
  for a knowledge skill Claude should auto-apply, removes
  `disable-model-invocation`. `name` is omitted (defaults to the directory) so
  it cannot drift from the folder. `harness validate skills` verifies the
  result — and the new skill is a first-class promotion/retirement target the
  lifecycle + governance loop already recognizes (`.claude/skills/**`).
- **`extend permission deny <pattern>`** — append `<pattern>` to the
  `permissions.deny` array in `.claude/settings.json`. The pattern must
  follow the spec grammar (`Bash(cmd *)`, `Read(path)`, `Edit(path)`,
  `Write(path)`, `WebFetch(domain:...)`, `mcp__server[__tool]`).
- **`extend permission ask <pattern>`** — same, into `permissions.ask`.
- **`extend permission allow <pattern>`** — same, into `permissions.allow`.
  Refuse when `<pattern>` is a read-only built-in (`ls`, `grep`, `cat`,
  read-only `git`) — its allow rule is a no-op.
- **`extend language <lang>`** — bootstrap a new language directory with the
  three `{lang}` templates `scaffold.toml` names —
  `permissions.allow.json`, `post-format.sh`, `rules/<lang>-conventions.md` —
  plus the matching `<lang>-dev` profile stub in `profiles.rs`. The hook
  wrappers, `session-start.sh`, and `check-on-stop.sh` are foundation-tier and
  never per-language. Operator fills the toolchain commands; the
  `scaffold_manifest` and `policy_template_sync` reverse-gap tests enforce
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
    overwriting. Map CI stages to gates if CI config exists. Drop any phase
    whose artifact nobody on this project would review and no later session
    would read — the pipeline is checkpoints and state, and a phase that is
    neither is ceremony the project pays for on every spec. `specs/_template/`
    holds one file per artifact-producing phase, so a pipeline customized in
    the rule is customized there in the same install — a phase added gains its
    template file, a phase dropped loses it. The `<...>` placeholders inside
    those files are filled per spec by whoever starts one and are not
    install-time fill markers.
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
  - `write-guard` — detect files with lifecycle governance (docs/, specs/
    with status frontmatter). Detect existing convention checking tools
    (linter config, type checker). Pre-fill the verifier's case arms with
    observed protection patterns. Add a PreToolUse(Edit|Write) hook entry
    to `.claude/settings.json` dispatching through `_runner.sh`.

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
  - `write-guard` — PreToolUse(Edit|Write) enforcement: verifier
    skeleton + governance rule for write-time convention checking.

In every verb: read the module-map's `existing_harness` first; match the
incumbent hook-runner pattern, rule mechanism, and gate sequence. Never
overwrite `settings.json` top-level keys outside the verb's scope.

## Mode: audit (read-only, gap report)

Two halves, split by what is decidable. The binary decides what is provably
wrong; the skill judges what is missing, because "missing" depends on what
the project already guarantees elsewhere and no binary can see that.

**1 — Drive `harness audit`** and present its `AuditOutcome` envelope. Every
finding is a defect against the spec or against the harness's own wiring:
- `audit-ms-timeout` — hook timeout values that look like milliseconds
  (≥ 1000) instead of seconds.
- `audit-mcp-matcher-incomplete` — `mcp__server` matcher without the
  required `__.*` suffix (matches nothing).
- `audit-hook-script-missing` — a hook names a `${CLAUDE_PROJECT_DIR}` path
  that is a scaffold-manifest destination and is not on disk, so the handler
  errors and the action proceeds unguarded. Scoped to manifest destinations
  on purpose: the anchor proves a token is a project path, never that the
  project has already built it, and `node_modules/.bin/*` or `target/release/*`
  are correct wirings that are simply absent on a fresh clone. The cost is
  that this protects harnex-generated wiring, not an operator's own scripts.
- `audit-managed-region-edited` — content inside a `harnex-managed`
  region diverges from the corresponding template.
- `audit-managed-region-missing` — a managed artifact is on disk with its
  `harnex-managed` sentinels gone, so regenerate has nothing to write into.

**2 — Run the exploration Phase-1 fingerprint** (exploration.md) and compare
its `existing_harness` block against what a scaffold for the detected stack
would emit. Report each difference as an observation with its enforced-vs-
advisory tier, never as a defect: a project may hold the same guarantee
server-side (CI secret scanning, a pre-receive hook, managed settings), and
harnex cannot see that from the repo. Name the guarantee, name where harnex
would put it, and let the operator decide. State plainly when a gap is one
harnex has no template for — an unsupported stack, an artifact class outside
the template set — rather than proposing a substitute.

Write nothing in either half.

## Mode: regenerate (spec drift)

Re-derive the managed regions against the CURRENT spec-facts (the case a
frozen binary cannot serve). For each file with sentinel markers:
1. Read the existing file. Extract project-authored regions (everything
   outside managed sentinels).
2. Render the managed regions fresh from the current template + language
   profile.
3. Write the file back with project-authored regions preserved verbatim.

For `.claude/settings.json`: re-derive only the harnex-owned ENTRIES (the
baseline + `<lang>-dev` permission rules; the base SessionStart/PostToolUse/Stop
hook entries) and MERGE — never drop a permission rule or hook entry harnex did
not author (operator `extend` additions and incumbent hand-rolled entries must
survive). On a conflict (an incumbent entry occupies a base slot with different
content), surface it for operator review rather than overwrite. Preserve every
other top-level key (`autoMemoryEnabled`, `skillOverrides`, `env`, etc.).

Report what changed and why.

## Verify before finishing

Generated shell hooks pass `bash -n`; generated JSON parses; the harness the
skill emits would itself pass `harness check` / `harness validate settings`
/ `harness audit`. For UI-less generation, state what was emitted and what
the operator must run.
