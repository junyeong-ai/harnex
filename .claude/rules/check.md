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
4. Document the slug in this rule.

A validator that is not glob-driven (`validate.settings` reads two named
files with independent `--since` status) keeps its own method and says why.

Validator slugs (current):
- `validate.rules`
- `validate.skills`
- `validate.agents`
- `validate.output_styles`
- `validate.settings`
- `evidence`
- `governs`
- `codegen`
- `policy.permissions`

The `governs` arm shares the rule validator's gate (`[validate.rules]`) and
glob: shape findings are the validator's, and this arm asks only the
existence question that needs the tree (`governs-truth-missing`). Like
codegen it ignores `--since` — the defect is created by a change to a
declared truth, not to the rule declaring it, so a diff-windowed rule filter
reads a deleted truth as nothing-to-check — and it counts nothing into
`files_scanned`.

Each validator that has no config section is added to `skipped` with the
reason "no [section] section" — never silently absent.

The `codegen` validator ignores `--since` by design: a sentinel source
edit can drift any target, so it always checks every configured group in
full. `validate.settings` filters `settings.json` and `settings.local.json`
independently — a change to one is never masked by the other.

`--since <ref>` filtering uses `git diff --name-only <ref>`. When `git`
fails to resolve the ref, the entire check surfaces `CheckGitFailure`
— never silently degrades to scanning everything.

Codegen drift is reported with `auto_fixable: true` and
`fix_command: Some(FixCommand::CodegenSync)` — downstream
agents (CI, pre-commit) can execute the fix without operator intervention.

`harnex check --fix` (and `ProjectChecker::fix`) close the loop: groups
findings by `fix_command`, dispatches each through the [`FixCommand`]
enum's exhaustive match in `try_fix`, then re-runs the check. Returns
`FixReport { before, fixes_attempted, after }` — the consumer compares
`before.findings.len()` vs `after.findings.len()` to confirm convergence.

Adding a new auto-fixable finding requires three coordinated edits:
1. Add a `FixCommand` variant + its `as_str()` arm (the enum is the
   single source of truth — exhaustive match enforces sites 2+3).
2. Emit the finding with
   `fix_command: Some(FixCommand::X)`. The field is typed, so this
   step is the compiler's, not a review's.
3. Add a match arm in `ProjectChecker::try_fix` — the compiler enforces
   this is exhaustive across `FixCommand` variants.
4. Add a test that asserts convergence (drift before → 0 findings after).

The registry is intentionally an enum, not config-driven, because
spawning arbitrary commands would defeat the safety invariant. Every
fix branch is reviewed code.
