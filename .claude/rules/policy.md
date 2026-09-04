---
paths:
  - "crates/harness-core/src/policy/**"
governs:
  concept: permission rule grammar and profiles
  live_truth:
    - crates/harness-core/src/policy
---

# policy — permission rules, profiles + versions

Permission profiles are static data. Each `PermissionProfile` carries
`name`, `allow`, `ask`, `deny`. Composition is set-union with sort+dedup.

When adding a new profile:
1. Add a `fn <name>() -> PermissionProfile` in `policy/profiles.rs`.
2. Add a match arm in `PermissionProfile::from_str`.
3. Append the name to `PermissionProfile::ALL`
   ([file: crates/harness-core/src/policy/profiles.rs :: pub const ALL] — single source of truth;
   the round-trip test catches drift).
4. Document its scope in the function comment (which ecosystem hazards it covers).

Profile naming is by scope, not by tool: `baseline` is the deny floor
(OS-universal hazards) and `workspace` the allow floor (working in a
repository at all — neither carries a language dependency);
`<ecosystem>-strict` is a cloud/tool surface; `<lang>-dev` is a language
toolchain and carries nothing but that toolchain — the floor belongs to
`workspace`, or a stack with no language profile scaffolds with an empty
allow list.

A `<lang>-dev` profile is the ecosystem's mainstream toolchain, not a claim
about which of it a given project uses. An allow for an absent tool never
matches; a missing one prompts on every invocation. The project's own gate
driver is composed on evidence by the skill, never guessed here.

`profiles.rs` is the single source of truth for permission rules. The harnex
plugin's committed permission templates are a projection of it:
`templates/common/permissions.deny.json` mirrors `baseline.deny`,
`templates/common/permissions.allow.json` mirrors `workspace.allow`, and each
`templates/<lang>/permissions.allow.json` mirrors `<lang>-dev.allow`. The
`policy_template_sync` integration test fails on any drift, and holds the
foundation and language allow sets disjoint. After editing a profile,
regenerate the matching template (`harnex policy permissions generate` with
that profile selected) and copy the array across — never hand-edit one side.
A new `<lang>-dev` profile MUST ship its template.

Rule grammar has one owner: `policy/rule.rs`. Ask `PermissionRule::effect`
whether a permission check reads a rule, and `PermissionRule::misleading`
whether its reach differs from its reading (the legacy `:*` suffix; a tail
after a wildcard on the allow side) — never match on the rule string.
Bash uses space-then-`*` (`Bash(cmd *)`); never grant built-in read-only
commands (no-op); a Read deny already covers `cat`/`head`/`tail`/`sed`, so
emit no `Bash(cat …)` mirror.

A rule Claude Code accepts and never consults is refused at every boundary
one can be written: `every_profile_rule_is_consulted` for a profile,
`Config::validate` for `[policy.permissions]` extras, `SettingsValidator`
for a settings file. After changing a vocabulary in `rule.rs`, re-read
/en/permissions, then move the `permissions` stamp in `spec.rs` and the
mirrored blocks in `spec-facts.md`.

Version strategies (`exact`/`minor`/`major`/`rolling`) are the only
permitted values; `Config::validate` rejects others. The checker never
spawns subprocesses to learn installed versions — callers pipe the
version string into `check_installed`.
