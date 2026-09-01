# Per-pattern analysis instructions (`extend pattern <name>`)

Read the entry for the pattern being installed, after the manifest entry and
skeleton (`SKILL.md § extend pattern` owns the flow; this file owns only what
each pattern's Step-2 analysis must observe). Every instruction here is about
reading the target project — a fill resolved from the model's priors instead of
an observation is the blank-page problem in disguise.

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
  `.claude/rules/` files, as `rule:<slug>`. Customize each lens's
  `applies_to:` based on what file types the project has. Name this project's
  **authorities** in the rule's source column: a finding may only be
  auto-fixed when something other than the loop confirms it, so the lint
  codes, named gates and structural tests are what decide the fix/report
  split. Take them from the enforcer sweep (exploration Phase 2) — it is the
  same list, and it also supplies the **bookend trigger's project signals**
  (a migration surface, an auth path, a generated-file guard). Name the
  **fast gate command** the loop's verify step runs and grant it in the
  review skill's `allowed-tools`.
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
  starts one and are not install-time fill markers. `check-plan.sh` lands in
  `hooks/pre-commit.d/`, is made executable, and its `PLAN_GLOB` follows the
  layout the analysis observed — the scaffold's pre-commit dispatches every
  arm there, which is how the review floor gets a commit-time computer
  without editing a byte-identical hook.
- `telemetry-kinds` — verify the scaffold's `harness.toml` declares the
  `harness_invocation` Kind exactly as `common/harness.toml` ships it, and add
  it if a brownfield `harness.toml` lacks it — the emit no-ops silently on an
  undeclared Kind, so the pattern would install looking enabled and record
  nothing. Do not invent a second Kind. In the same edit, set
  `invocation_kind = "harness_invocation"` on each `[[kinds]]` whose slugs this
  record can name — the skill and sub-agent kinds, never a rule kind, whose
  artifacts are loaded rather than invoked and so appear in no invocation
  record. That is what retirement measures silence against; a kind without it
  stays `unmeasured` however full the ledger gets, and a kind wrongly given it
  has every artifact convicted as soon as anything else runs. Wire `hooks/telemetry-emit.sh` as two
  `.claude/settings.json` entries — `PostToolUse` and `PostToolUseFailure`,
  matcher `Skill|Task|Agent` (the tools that invoke a harness element; its slug
  is what the retirement sweep reads — MCP tools are not harness elements and
  are deliberately not recorded), with `async: true` and a short `timeout` so
  the append never sits on the tool's critical path. Dispatch it through
  `_runner.sh`, the same as the other session hooks: the runner execs the
  wrapper via `bash`, so the template ships without an executable bit and a
  directly-wired `0644` script cannot fail with a permission error. The wrapper
  delegates to `harnex guard telemetry-emit`, which does its own `harness.toml`
  discovery and owns the tool→element mapping, the outcome derivation, and every
  silent skip; the wrapper adds only that an absent oracle is a silent exit 0.
  It is install-to-enable and silent without the oracle. State whether the
  retirement sweep should read this ledger; only the element's slug and the
  outcome ever cross.
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
- `routines` — schedule the first tick of each shipped routine (`when:` +
  `produces:`) or leave them deliberately unscheduled and say so — the
  session surface reports `unscheduled` loudly until they are. Pick the
  records directory the `produces:` paths land in from where the project
  keeps long-lived records. Wire `hooks/session-routines.sh` as a
  SessionStart hook entry in `.claude/settings.json`, alongside the
  scaffold's own; it is install-to-enable and silent without the oracle.
- `enforcement-floor` — read `[guard.floor] protected_paths` off the
  project's own gates: the git hooks directory, the secret-scan config, the
  configs its linters and formatters read, and the sources of any verifier a
  hook dispatches (the enforcer sweep already lists them). harness.toml and
  the two settings files are built into the floor — never list them. Wire
  two PreToolUse entries in `.claude/settings.json` invoking `harnex guard
  floor` directly (matchers `Bash` and `Edit|Write|MultiEdit`, stdin passed
  through, no `_runner.sh`). A missing `[guard.floor]` section skips with a
  visible notice; a missing binary errors on every call — deliberately not
  silenced, because enforcement that dies quietly is the failure the floor
  exists to catch, so this pattern requires the oracle installed. Tell the
  operator the break-glass entry by name; it is theirs, not the agent's.
