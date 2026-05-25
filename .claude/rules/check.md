---
paths:
  - "crates/harness-core/src/check.rs"
---

# check — unified validation gate

`ProjectChecker::run` is the single entry point for "run every validator
this config enables". Findings are aggregated under one envelope with
deterministic sort order: severity ascending (Blocker first), slug,
path. The shape never changes — adding a new validator only extends the
list of producer slugs.

When adding a new validator:
1. Add a `run_<name>` private method on `ProjectChecker` that takes
   `(&changed, &mut findings, &mut run, &mut skipped, &mut files_scanned)`
   and follows the same skipped-vs-ran contract.
2. Call it from `ProjectChecker::run` between existing validators.
3. Include the validator's slug in the test
   `check_runs_every_enabled_validator` `run` assertion list.
4. Document the slug in this rule.

Validator slugs (current):
- `validate.rules`
- `validate.skills`
- `validate.settings`
- `evidence`
- `codegen`
- `policy.permissions`

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
`fix_command: FixCommand::CodegenSync.as_str().into()` — downstream
agents (CI, pre-commit) can execute the fix without operator intervention.

`harness check --fix` (and `ProjectChecker::fix`) close the loop: groups
findings by `fix_command`, dispatches each through the [`FixCommand`]
enum's exhaustive match in `try_fix`, then re-runs the check. Returns
`FixReport { before, fixes_attempted, after }` — the consumer compares
`before.findings.len()` vs `after.findings.len()` to confirm convergence.

Adding a new auto-fixable finding requires three coordinated edits:
1. Add a `FixCommand` variant + its `as_str()` arm (the enum is the
   single source of truth — exhaustive match enforces sites 2+3).
2. Emit the finding with
   `fix_command: Some(FixCommand::X.as_str().into())`.
3. Add a match arm in `ProjectChecker::try_fix` — the compiler enforces
   this is exhaustive across `FixCommand` variants.
4. Add a test that asserts convergence (drift before → 0 findings after).

The registry is intentionally an enum, not config-driven, because
spawning arbitrary commands would defeat the safety invariant. Every
fix branch is reviewed code.
