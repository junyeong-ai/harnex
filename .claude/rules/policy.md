---
paths:
  - "crates/harness-core/src/policy/**"
---

# policy — permissions + versions

Permission profiles are static data. Each `PermissionProfile` carries
`name`, `allow`, `ask`, `deny`. Composition is set-union with sort+dedup.

When adding a new profile:
1. Add a `fn <name>() -> PermissionProfile` in `policy/profiles.rs`.
2. Add a match arm in `PermissionProfile::from_str`.
3. Append the name to `PermissionProfile::ALL` (single source of truth —
   the round-trip test catches drift).
4. Document its scope in the function comment (which ecosystem hazards it covers).

Profile naming: `<ecosystem>-strict` for cloud/tool surfaces; `baseline`
for OS-universal hazards only; `<lang>-dev` for a language toolchain.

`profiles.rs` is the single source of truth for permission rules. The harnex
plugin's committed permission templates are a projection of it:
`templates/common/permissions.deny.json` mirrors `baseline.deny`, and each
`templates/<lang>/permissions.allow.json` mirrors `<lang>-dev.allow`. The
`policy_template_sync` integration test fails on any drift. After editing a
profile, regenerate the matching template (`harness policy permissions
generate` with that profile selected) and copy the array across — never
hand-edit one side. A new `<lang>-dev` profile MUST ship its template.

Rule grammar follows the Claude Code spec: Bash uses space-then-`*`
(`Bash(cmd *)`); never grant built-in read-only commands (no-op); a Read deny
already covers `cat`/`head`/`tail`/`sed`, so emit no `Bash(cat …)` mirror.

Version strategies (`exact`/`minor`/`major`/`rolling`) are the only
permitted values; `Config::validate` rejects others. The checker never
spawns subprocesses to learn installed versions — callers pipe the
version string into `check_installed`.
