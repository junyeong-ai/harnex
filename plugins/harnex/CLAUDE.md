# harnex plugin

The single-skill plugin: `SKILL.md` (entry; the mode menu lives in SKILL.md) +
`reference/` (L1
knowledge) + `templates/` (L2 safety-critical templates). Editing contract
for this directory (the runtime content ships to installs; this file guides
editing it, not using it):

- **Compose templates; never free-generate** a hook, permission rule, or
  timeout. The skill selects a language profile and fills declared params.
- **Permission templates are a projection, not a source.** `permissions.deny.json`
  and `<lang>/permissions.allow.json` are generated from the oracle's
  `crates/harness-core/src/policy/profiles.rs` (`baseline` / `<lang>-dev`).
  Edit the profile, regenerate with `harness policy permissions generate`,
  copy the array across; the `policy_template_sync` test fails on drift
  (constitution IX). Never hand-edit a template's rules.
- **`reference/spec-facts.md` is perishable.** Re-verify each fact against the
  live Claude Code docs every change — a frozen spec fact is the failure mode.
  Closed-set vocabularies inside spec-facts (hook events, …) live in
  `<!-- harnex-managed:start <slug> -->` blocks that the `spec_facts_sync`
  integration test holds in lock-step with the Rust SSoT (constitution IX).
- **Managed-region convention for generated artifacts.** Markdown templates
  (`common/CLAUDE.md`, `common/rules/constitution.md`) carry
  `<!-- harnex-managed:start <slug> -->` / `<!-- harnex-managed:end <slug> -->`
  sentinels bounding the harnex-owned region. `regenerate` overwrites only
  inside sentinels; everything outside is project-authored. `.claude/settings.json`
  is JSON (no comments), so its partition is by top-level key:
  `permissions` and `hooks` are harnex-managed; every other key is
  project-owned and must survive regenerate.
- **Budgets:** `SKILL.md` body < 500 lines; `description` + `when_to_use`
  ≤ 1536 chars, key use case first.
- **`templates/scaffold.toml` is the composition.** Which artifacts a harness
  contains, where each lands, which tier it belongs to, and how the project's
  copy relates to its template (`content.kind`: `copy` | `seed` | `managed` |
  `merge`) are declared there and nowhere else. The skill emits from it, the oracle's fixture test builds
  from it, and `harness audit` reports coverage against it, so a file list
  restated in prose is the one that drifts (constitution IX). The `foundation`
  tier is what a stack with no language profile still receives; nothing in it
  may reference a `language`-tier artifact, and `scaffold_manifest` fails if
  one does.
- **Add a language** = the three `{lang}` templates the manifest names
  (`permissions.allow.json`, `post-format.sh`, `rules/<lang>-conventions.md`)
  plus a `<lang>-dev` profile in the oracle AND a row in
  `reference/language-matrix.md` (detection fingerprint + parameters). The
  manifest itself never changes — `{lang}` resolves against
  `PermissionProfile::ALL`, and `scaffold_manifest` fails in both directions:
  a profile without templates, and a template no artifact emits. There is no
  per-language runner: `common/_runner.sh` dispatches every verifier and each
  non-shell arm probes its own interpreter.
- **Add a pattern** = a `templates/patterns/<slug>/` directory with the
  skeleton files + a `[[pattern]]` entry in `templates/patterns/manifest.toml`
  (slug, files, analyze steps). The `pattern_manifest_sync` test fails on
  drift between manifest and directories. Pattern files ship CONCRETE proven
  defaults, never blank fill-ins — every `<!-- Fill in -->` is replaced at
  install time by the skill from project analysis.
- **Extend mode is a closed verb menu.** When adding a new extend verb,
  add its bullet to the `## Mode: extend` menu in `SKILL.md`, add a tested
  composition path in templates, and (if the verb mutates a SSoT) extend
  the matching audit check. Free-form extension invites free-generation.
