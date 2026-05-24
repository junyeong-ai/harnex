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
   exit 0, `mcp__server` / `mcp__server__tool` matchers (never a regex form),
   `Bash(cmd *)` space-wildcards, `deny>ask>allow`, no project-ignored settings
   keys. When in doubt, re-read the live doc — freezing the spec is the failure.
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
1. Phase-1 fingerprint (exploration.md) → language profile. Single-package if no
   workspace globs — emit the lean variant (no per-module layer).
2. Compose into `${CLAUDE_PROJECT_DIR}`:
   - `.claude/settings.json` (`permissions` = common `permissions.deny.json` +
     `<lang>/permissions.allow.json`; `hooks` = common `hooks.json`)
   - `hooks/` (`<lang>/_runner.sh`, common `_stop_runner.sh`,
     `<lang>/post-format.sh`, `<lang>/session-start.sh`,
     common `check-on-stop.sh`)
   - `.claude/rules/constitution.md` (common, managed region wraps the
     articles — the path-scoped rules added later sit beside it untouched)
   - `CLAUDE.md` (common skeleton; user fills `## Layout`, `## Build & test`,
     `## Conventions` — they are project-authored; the `## Enforcement`
     section is the managed region)
   - Optionally one `<lang>/rules/<lang>-conventions.md` as a starting
     path-scoped rule example.
3. Set hook scripts executable (0o755). Run `harness check` if the binary
   oracle is available.

## Mode: extend (brownfield, additive — closed verb menu)

Free-form additive generation invites free-form free-generation. The verb
menu below enumerates the closed set; refuse any other request and ask the
operator to re-phrase using a verb from this list.

- **`extend hook <event-name>`** — add a hook for `<event-name>` (must be
  in spec-facts hook events). Compose `_runner.sh` dispatch + a new verifier
  script next to the existing siblings; add the event entry to
  `.claude/settings.json` `hooks` (the managed region).
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
  `templates/<lang>/{_runner.sh, post-format.sh, session-start.sh,
  permissions.allow.json}` + the matching `<lang>-dev` profile stub in
  `profiles.rs`. Operator fills the toolchain commands; the
  `policy_template_sync` reverse-gap test enforces both sides exist.

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
`hooks` keys; preserve every other key (managed CLAUDE.md content,
`autoMemoryEnabled`, `skillOverrides`, etc.).

Report what changed and why.

## Verify before finishing

Generated shell hooks pass `bash -n`; generated JSON parses; the harness the
skill emits would itself pass `harness check` / `harness validate settings`
/ `harness audit`. For UI-less generation, state what was emitted and what
the operator must run.
