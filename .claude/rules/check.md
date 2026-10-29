---
paths:
  - "crates/harness-core/src/check.rs"
governs:
  concept: the unified validation gate
  live_truth:
    - crates/harness-core/src/check.rs
---

# check — unified validation gate

`ProjectChecker::run` is the single entry point for "run every validator
this config enables". Findings are aggregated under one envelope with
deterministic sort order: severity ascending (Blocker first), slug,
path. The shape never changes — adding a new validator only extends the
list of producer slugs.

When adding a new glob-driven validator:
1. Implement `harness_core::validate::SurfaceValidator` on it — the config
   accessor, the glob, and the slug. The shared `run_surface_validator` then
   supplies the skipped-vs-ran contract, the `--since` filter, and the file
   count; never copy that body.
2. Add one `run_surface_validator::<V>` call in `ProjectChecker::run`.
3. Include the slug in `check_runs_every_enabled_validator` and in
   `check_skips_validators_with_no_config_section`.

A validator that is not glob-driven (`validate.settings` reads two named
files with independent `--since` status) keeps its own method and says why.
The calls in `ProjectChecker::run` are the arm set; each records its slug in
`run` or `skipped`.

The `guard.floor` arm holds `sandbox.filesystem.denyWrite` in
`.claude/settings.json` to cover every path the floor freezes
(`floor-sandbox-uncovered`, the entry to add in its hint) except what the
sandbox refuses on its own (`sandbox::SANDBOX_PROTECTED`). The hint also names
the other way out: a `[guard.floor]` without the Edit|Write floor hook freezes
nothing and belongs out of `harness.toml` — the state of a harness scaffolded
before 0.30 that never adopted the enforcement-floor pattern. An absent
settings file hands the sandbox no entries; only one that is present and
unreadable leaves the arm unjudged in `skipped`. Like governs it ignores
`--since`: a path added to `[guard.floor]` breaks the projection as surely
as an entry dropped from the settings.

The `validate.always_loaded` arm is not a surface validator: it reads the
whole set `always_loaded::resolve` returns and reports one
`always-loaded-over-budget` past `max_chars`, located at the largest member.
It ignores `--since`, because every member is part of one sum.

The `agent-preload` arm shares the agent validator's gate (`[validate.agents]`)
and glob, and reports `agent-skill-not-preloadable` from
`validate::agents::audit_preload`. It reads a second file — the project skill
an agent's `skills:` names — so it lives here rather than in the per-file
validator, and like governs it ignores `--since`: a change to the skill alone
is what breaks an unchanged agent.

The `governs` arm shares the rule validator's gate (`[validate.rules]`) and
glob: shape findings are the validator's, and this arm asks only the
existence question that needs the tree (`governs-truth-missing`). It
ignores `--since` — the defect is created by a change to a
declared truth, not to the rule declaring it, so a diff-windowed rule filter
reads a deleted truth as nothing-to-check — and it counts nothing into
`files_scanned`.

The `paths-reading` arm shares the rule and skill validators' gates and globs
and reports `paths-reading-differs` (Minor) where dropping a trailing `/**`,
as Claude Code does, makes a `paths:` value cover different project files than
as written — `docs/**` once a nested `docs` directory exists. Both sides fold
case as Claude Code does, so a difference folding alone makes shows on
neither, and one the drop makes on a file spelled in another case still shows.
The hint names a rewrite for each glob whose dropped `/**` the value cannot
keep and still cover what it does as written, so applying them all clears the
finding; the every-depth spelling it offers beside one changes what the value
covers as written, so the value is judged again. It asks only the question that needs the tree, so a value outside the
grammar stays the validators' to report, and lists the project only once a
value drops a `/**`: without one the readings cannot part. The project's files
are what `git::owned_files` lists that is not a directory — a submodule and a
nested repository are listed as one — less a file whose path holds a line
terminator, which neither reading answers for (`path_globs::is_answerable`);
when git cannot list them, the arm is declared unmeasured in `skipped`. Like governs it ignores `--since` — a
directory added anywhere parts the readings of an unchanged rule — and counts
nothing into `files_scanned`.

The `evidence` arm reads every surface this gate validates for shape — the
rule, skill, agent, output-style and routine globs the validators discover
with — plus the project's memory files, because a file checked for
frontmatter while its claims go unchecked reads as verified.
It runs in full under `--since`: changing a cited target can invalidate a
claim in an unchanged document.
`check_reads_a_claim_from_every_shape_validated_surface` holds the list to the
validators `run` dispatches. A claim is project-relative wherever it is
written; a nested `CLAUDE.md` citing paths from its own directory resolves
against the wrong file or none, and the frame is the same one Claude Code's
own tools resolve in.

The memory set is the project memory files the runtime reads at launch
(`always_loaded::memory_files` owns which), unconditionally, plus every
nested `CLAUDE.md` the project owns — `harness-core::git` owns that set and
why it is the project's ignore files and not the developer's. A walk would
read the one a vendored package ships and resolve its paths here.
`claudeMdExcludes` in `.claude/settings.json` is honored for every memory
file — a `CLAUDE.md` and a rule — because one the runtime never loads makes no
claim; it is read through `always_loaded::Excludes`, which matches the
absolute path as the runtime does, so a relative pattern excludes nothing. A
pattern whose glob it cannot apply whole excludes only a path equal to it:
`validate always-loaded` lists it in `unread_excludes`, and `validate.settings`
names one written outside the dialect (`settings-exclude-unread`).
`settings.local.json` is the developer's, and a gate that read it would pass
a tree locally that CI fails. When git cannot answer — no repository,
dubious ownership — the nested set is declared unmeasured in `skipped` as
`evidence.nested-memory` and everything else is still read; a git failure
under `--since` stays `CheckGitFailure`, because that window was asked for.
The window is `--relative`: without it a config below the git top level is
answered from the wrong root, and a changed `apps/web/CLAUDE.md` is never the
candidate the gate discovered from its own directory.

The `advisory` arm (gated on `[evidence]`) holds each declared advisory's
recorded baseline fresh: digests of the declared inputs and engine against
what recording stamped, absence as `advisory-unmeasured`, an undeclared
baseline as orphaned. Same `--since` exemption and `files_scanned` silence
as governs, for the same cross-file reason. `--unattended` (wired through
`with_unattended`) downgrades staleness to Minor exactly where the entry
does not declare `unattended_remeasure`.

Each validator that has no config section is added to `skipped` with the
reason "no [section] section" — never silently absent.

`validate.settings` filters `settings.json` and `settings.local.json`
independently — a change to one is never masked by the other.

`--since <ref>` filtering uses `git diff --name-only <ref>`. When `git`
fails to resolve the ref, the entire check surfaces `CheckGitFailure`
— never silently degrades to scanning everything.
A changed or untracked `harness.toml` revalidates every surface from the
config's own root, because the validators' inputs include their policy.
