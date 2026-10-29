# Patterns (`extend pattern <name>`)

The patterns `extend pattern` offers, one entry per `[[pattern]]` in
`templates/patterns/manifest.toml` — `pattern_manifest_sync` holds the two to
one set, and the manifest owns each pattern's files. An entry opens with what
the pattern gives a project, then says what its Step-2 analysis must observe;
read it after the manifest entry and skeleton
([file: SKILL.md § Mode: extend (brownfield, additive — closed verb menu)]
owns the flow). Every instruction here is about reading the target project — a
fill resolved from the model's priors instead of an observation is the
blank-page problem in disguise.

- `review-lenses` — the convergent review loop and two forked read-only
  bookends (critique, design-review) as skills, the fresh-context reviewer
  agent they run through, the lens files, and the `review-lenses` skill under
  which a finding's citation decides whether it may be fixed unattended — it
  loads when a review starts, so a session that is not reviewing pays nothing
  for it. Auto-link lens
  `anchors:` to the project's existing
  `.claude/rules/` files, as `rule:<slug>`. Customize each lens's
  `applies_to:` based on what file types the project has. Name this project's
  **authorities** in that skill's source column: a finding may only be
  auto-fixed when something other than the loop confirms it, so the lint
  codes, named gates and structural tests are what decide the fix/report
  split. Take them from the enforcer sweep (exploration Phase 2) — it is the
  same list, and it also supplies the **bookend trigger's project signals**
  (a migration surface, an auth path, a generated-file guard). Name the
  **fast gate command** the loop's verify step runs and grant it in the
  review skill's `allowed-tools`. A project that installed this pattern
  before 0.36 holds the same vocabulary as `.claude/rules/review-lenses.md`
  with `review-lenses` in `[validate.rules] always_loaded_slugs`: carry its
  filled authorities and trigger signals into the skill, then delete the rule
  and the slug, or the vocabulary has two owners and still loads every
  session.
- `spec-workflow` — the spec orchestrator as a skill (gate events with a
  closed decision-token enum, resume from disk), the threshold-and-lifecycle
  rule, and `specs/_template/`. Check for an existing `specs/` or `docs/adr/` directory.
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
  starts one and are not install-time fill markers. Verify a brownfield `harness.toml`
  carries `[lifecycle]`, and add it if not: wrapup records each harness
  proposal to the observation ledger in the commit that retires the spec
  directory, so a pattern installed without it reaches the one step whose
  failure loses the proposals.
- `telemetry-kinds` — the closed-schema ledger the harness measures its own
  use with: a hook recording each Skill, Task and Agent invocation, and the
  rule on what may cross into it. Add the `harness_invocation` Kind to
  `harness.toml` exactly as the pattern's rule spells it — the emit no-ops
  silently on an undeclared Kind, so the pattern would install looking enabled
  and record nothing. Do not invent a second Kind. In the same edit, set
  `invocation_kind = "harness_invocation"` on each `[[kinds]]` whose slugs this
  record can name — the skill and sub-agent kinds, never a rule kind, whose
  artifacts are loaded rather than invoked and so appear in no invocation
  record. That is what retirement measures silence against; a kind without it
  stays `unmeasured` however full the ledger gets, and a kind wrongly given it
  has every artifact convicted as soon as anything else runs. Wire `hooks/telemetry-emit.sh` as two
  `.claude/settings.json` entries — `PostToolUse` and `PostToolUseFailure`,
  matcher `Skill|Task|Agent` (the tools that invoke a harness element; its slug
  is what the retirement sweep reads — MCP tools are not harness elements and
  are deliberately not recorded), with `async: true` so the append never sits
  on the tool's critical path. Dispatch it through
  `_runner.sh`, the same as the other session hooks: the runner execs the
  wrapper via `bash`, so the template ships without an executable bit and a
  directly-wired `0644` script cannot fail with a permission error. The wrapper
  delegates to `harnex guard telemetry-emit`, which does its own `harness.toml`
  discovery and owns the tool→element mapping, the outcome derivation, and every
  silent skip; the wrapper adds only that an absent oracle is a silent exit 0.
  It is install-to-enable and silent without the oracle. State whether the
  retirement sweep should read this ledger; only the element's slug and the
  outcome ever cross.
- `routines` — recurring harness work with a cadence and a record: hygiene
  routines, the rule they follow, and a SessionStart hook that reports a
  routine overdue or never scheduled. Schedule the first tick of each shipped routine (`when:` +
  `produces:`) or leave them deliberately unscheduled and say so — the
  session surface reports `unscheduled` loudly until they are. Pick the
  records directory the `produces:` paths land in from where the project
  keeps long-lived records. Wire `hooks/session-routines.sh` as a
  SessionStart hook entry in `.claude/settings.json`, alongside the
  scaffold's own; it is install-to-enable and silent without the oracle.
- `enforcement-floor` — the freeze that keeps the Edit tools off the files
  defining the project's gates, and its rule. The tripwire beside it is
  already wired by every scaffold (`hooks/check-floor.sh`, matcher
  `Bash`), so add one entry and never a second `Bash` one. Read
  `[guard.floor] protected_paths` off the project's own gates: the git hooks
  directory, the secret-scan config, the configs its linters and formatters
  read, and the sources of any verifier a hook dispatches (the enforcer sweep
  already lists them). harness.toml and the two settings files are built into
  the floor — never list them. Add the `sandbox.filesystem.denyWrite` entry
  `harnex check` names for each frozen path; what the sandbox already
  refuses on its own, the settings files among it, needs none. Wire one
  PreToolUse entry in `.claude/settings.json` running
  `${CLAUDE_PROJECT_DIR}/hooks/check-floor.sh` with matcher
  `Edit|Write|MultiEdit`. Offer it only where an agent has been seen weakening
  a gate to get a change through — a reverted or corrected edit to a gate file
  in the history, or a correction `harnex session facts` reports — and editing
  the gate files is not the project's ordinary work. Without that evidence the
  tripwire is the floor: every gate edit would need the grant, a grant left
  standing to spare that prints its notice on every one, and the one signal the
  freeze was bypassed stops being one. Tell the operator the
  break-glass entry by name, and that the `denyWrite` entries hold only in a
  session the sandbox runs in; both the grant and turning the sandbox on are
  theirs, not the agent's.
