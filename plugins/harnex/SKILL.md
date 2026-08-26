---
name: harnex
description: Generate and maintain project-fit, project-native Claude Code harness tooling — hooks, settings.json, CLAUDE.md, path-scoped rules — in the target project's own language, from verified spec-correct templates. Use to scaffold a harness in a fresh repo, extend one with a closed-verb additive change, audit an existing harness for spec drift, or regenerate the managed regions against the current Claude Code spec.
disable-model-invocation: true
argument-hint: "scaffold | extend <verb> <args> | retire <verb> <args> | audit | regenerate"
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
- `${CLAUDE_SKILL_DIR}/reference/retire.md` — the removal contract: what
  the evidence supports, and where it stops short of a verdict.

Templates live under `${CLAUDE_SKILL_DIR}/templates/`: language-agnostic
pieces in `common/`, and one directory per supported language
(`typescript/`, `python/`, `rust/`, `jvm/` today — adding a language is a new
`<lang>/` directory plus its `*-dev` permission profile). Generated files are
written to `${CLAUDE_PROJECT_DIR}` (the target repo).

Measuring how the operator instructs Claude Code is `/harnex:measure`, a
command outside this skill. Section 4 of its report is what turns a constraint
supplied by hand every session into an `extend` verb.

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
   **foundation tier** — every artifact it marks language-agnostic, read from
   the manifest rather than from a list here — and report exactly which
   language-tier artifacts are unavailable and why. What is forbidden is a
   *wrong* profile,
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
   entries it generated — the baseline + `workspace` + `<lang>-dev` permission
   rules and the base hook entries, each identified by its template shape
   (event + matcher + runner script). Read those shapes from
   `templates/common/hooks.json` and `templates/{lang}/hooks.format.json`
   rather than from a copy here: a matcher restated in prose is the one that
   goes stale, and a stale one makes regenerate read harnex's own entry as
   project-owned and write a second beside it. Entries an operator added via `extend`, and any incumbent
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
`content.key` as a **recursive** union: objects merge key-wise at every depth,
arrays gain the elements they lack in the order they arrive, an empty slot
takes the fragment whole, and where two shapes disagree that node is left
exactly as it was and the collision is reported — the siblings around it still
merge, so one malformed event does not withhold the rest (the manifest header states the rule and why depth is
not optional — `hooks` is an object whose values are arrays). `chmod 0o755` where `executable` is set. Emit the
`foundation` tier always, and the `language` tier once per detected stack,
resolving `{lang}` to that language each time. The manifest is the single home
for that set — a second list here would be the one that drifts, and the
oracle's fixture test builds from the same file so a scaffold and its guard
cannot disagree.

**Check every destination before writing it.** A repo with no `.claude/` — the
repo scaffold mode is for — usually still has a `CLAUDE.md`, because Claude
Code reads one without any `.claude/` directory, and a repo with git hooks
already has `hooks/`. An existing file is the project's: keep it, emit nothing
there, and say so. `merge` is the only kind that writes into an occupied
destination, because it never owns one. Report every collision you left in
place, naming the template the operator can merge from by hand.

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
Report the enforced invariants Phase 2 found — the guards this project already
runs that no rule names yet. Each is an `extend rule <slug> <paths-glob>` worth
making, and naming them is what turns a floor into this project's harness.
Suggest them; do not write them unasked.

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
- **`extend rule <slug> <paths-glob>`** — derive a path-scoped rule at
  `.claude/rules/<slug>.md` from `common/rule-template.md`, filling it from
  what the code under `<paths-glob>` **already enforces**. A skeleton handed to
  the operator is the blank page this skill exists to avoid, and a rule written
  from the model's priors is worse — it states confident things about a
  codebase nobody checked.

  Read four sources under the glob, in this order (exploration Phase 2's
  *enforced invariants* row):
  1. **Enforcers that already run over these paths** — CI steps, task-runner
     targets, pre-commit entries, `.claude/settings.json` hook verifiers,
     project lint scripts.
  2. **Structural invariants in the code** — a closed enum or registry with an
     exhaustive match, an allowlist, a base type every member implements, a
     single validation boundary, a custom error hierarchy.
  3. **Tests that assert structure rather than behavior** — one that enumerates
     members, holds two representations in sync, or pins a naming shape. A
     test is an invariant someone already decided was worth guarding.
  4. **What the formatter, linter and type checker already cover** — to
     *exclude*. The governance rubric rejects a rule that restates them.

  Then one hard rule: **an invariant with no enforcer in the tree does not
  become a rule.** It becomes `harness lifecycle observe --tag <slug> --text
  "<observation>"`, which is where a candidate waits until it has recurred.
  This is what keeps derivation from becoming invention, and it is not a
  matter of judgment — no enforcer, no bullet.

  Every bullet names its enforcer as a marked claim — `[file: path/to/x.py:42]`,
  the line optional — so the evidence
  check resolves it and a rename fails the gate instead of leaving a rule that
  points nowhere. Report what you moved to the ledger as well as what you
  wrote; the observations are usually the more interesting half.
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
  `WebFetch(domain:...)`, `Tool(param:value)`, `mcp__server[__tool]`).
  `harness validate settings` rejects a rule no permission check consults, so
  the pattern is verified rather than trusted.
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
  generic defaults with project-specific observations. Every
  `<!-- harnex-fill: … -->` marker MUST be replaced — with an observed value,
  or an explicit "none observed yet — <default behavior>" note. That token is
  the only marker the templates carry and the only one `harness audit` reads;
  a marker may wrap across lines, so search for `harnex-fill` rather than for
  a whole one-line comment. Never leave one in a generated file: a placeholder
  that ships is the blank-page problem in disguise, and
  `audit-fill-marker-unresolved` reports each survivor with the line and what
  it asked for.

  **Per-pattern analysis instructions:**
  - `naming-decisions` — every section is read out of the repository, never
    chosen for it: count file-name casing per kind, read the construction verbs
    off the functions that build things, read the suffixes off the option/config
    types, and take the domain vocabulary from type and table names rather than
    from prose. Name the file that settles each answer. Where the code is
    inconsistent, say which concept has two names — that is the decision the
    team still owes, and it is the most valuable line in the file. A convention
    imported from another project contradicts the code a reader is looking at,
    and the code wins.
  - `copy-conventions` — detect locale from string literals. Detect error
    message format from existing error handling code. Detect i18n framework
    from dependencies (next-intl, react-i18n, gettext, fluent). Pre-fill
    register and terminology with observations.
  - `review-lenses` — auto-link lens `anchors:` to the project's existing
    `.claude/rules/` files. Customize each lens's `applies_to:` based on what
    file types the project has. Name this project's **re-runnable authorities**
    in the rule: a finding may only be auto-fixed when something other than the
    loop confirms it, so the list of lint codes, named gates and structural
    tests is what decides the fix/report split. Take it from the enforcer sweep
    (exploration Phase 2) — it is the same list.
  - `spec-workflow` — check for existing `specs/` or `docs/adr/` directory.
    If found, adapt to the existing layout instead of overwriting. Drop any
    phase whose artifact nobody on this project would review and no later
    session would read — the pipeline is checkpoints and state, and a phase
    that is neither is ceremony paid on every spec. `specs/_template/` holds
    one file per artifact-producing phase, so a phase dropped loses its
    template file in the same install; the orchestrator derives the phase from
    which of them exist, so the two cannot disagree. Its fills each need a
    judgment rather than a lookup — the **blast-radius signals** that fire the
    design_review gate (take them from the enforcer sweep: a migration surface,
    a wire contract with a parity gate, an auth or tenancy path, a
    generated-file guard), **where a retired spec's learning lands** (an ADR
    directory, a learnings folder, or the commit body when the project keeps
    neither), and **what the review gate delegates to** (the review skill when
    that pattern is installed, otherwise this project's own review command).
    Resolve every marker the pattern ships, not a count stated here. The `<...>`
    placeholders inside the template files are filled per spec by whoever
    starts one and are not install-time fill markers.
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
  - `review-lenses` — the convergent review loop as a **skill**, a
    fresh-context reviewer **agent** for its terminal pass, the severity ×
    citation rule that decides what may be fixed unattended, and 6 lens files.
  - `spec-workflow` — the spec orchestrator as a **skill** (four gate events, a
    closed decision-token enum, resume from disk), the threshold-and-lifecycle
    rule, and `specs/_template/`.
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
- `audit-hook-not-executable` — a hook spawns a scaffold artifact directly
  (the entry carries `args`, so the runtime runs `command` itself with no
  shell) and the file's exec bit is off. Every invocation of that hook fails
  before the script starts, so the wrapper's own fail-open never runs. Remedy
  is `chmod +x`; `scaffold` Step 3 exists to keep this from happening.
- `audit-managed-region-edited` — content inside a `harnex-managed`
  region diverges from the corresponding template.
- `audit-managed-region-missing` — a managed artifact is on disk with its
  `harnex-managed` sentinels gone, so regenerate has nothing to write into.
- `audit-copy-drift` (Minor) — a `copy` artifact whose bytes differ from the
  template that emits it. Usually this is the project's own file at a
  destination the scaffold claims: it was kept, correctly, and the hook
  fragments that name that path were merged anyway, so a Claude Code event now
  dispatches to a script harnex did not write. Report it with the three states
  that produce it — kept incumbent, edited copy, older plugin version — because
  the bytes cannot tell them apart and only the operator knows which.
- `audit-fill-marker-unresolved` — a `<!-- harnex-fill: … -->` the generating
  step left behind. Expect several on any harness scaffolded without the Step-1
  analysis: `CLAUDE.md` and every `<lang>-conventions.md` ship them, and each
  names the observation the file is waiting for.

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

## Mode: retire (evidence-presented, operator-decided removal)

`extend` adds and nothing removed, so a harness only ever grew. This mode is
the other direction. It does not decide: the transcript records what a harness
element cost and never what it bought, so `retire.md` presents evidence and the
operator supplies the reason. Read it before running this mode.

- **`drop-hook <command>`** — a Stop hook's `runs`, `total_ms` and
  `stops_with_prevention`, with that field's attribution limit stated.
- **`drop-rule <path>`** — a rule absent from `rule_loads` across a window that
  did observe this project.

Run `harness session facts` first; evidence the envelope does not carry does
not exist. Then, for the chosen verb:

1. Present the evidence with its limit. Never call an element useless because
   nothing recorded it being useful.
2. Take the operator's decision text. Without it, stop — this is the same
   refusal `harness lifecycle` makes.
3. Locate the entry by the rendering rule in `retire.md`. No match or more than
   one → report and stop.
4. Refuse anything outside the managed partition (§ Invariants 6) and report
   where it lives instead.
5. Remove the entry, leaving every sibling and every other key intact.
6. Record the decision with `harness lifecycle`, then commit that removal alone.

Verify as `regenerate` does: the settings file still parses and still passes
`harness validate settings`.

## Verify before finishing

Generated shell hooks pass `bash -n`; generated JSON parses; the harness the
skill emits would itself pass `harness check` / `harness validate settings`
/ `harness audit`. For UI-less generation, state what was emitted and what
the operator must run.
