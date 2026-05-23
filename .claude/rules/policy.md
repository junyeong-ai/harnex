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
for OS-universal hazards only.

Version strategies (`exact`/`minor`/`major`/`rolling`) are the only
permitted values; `Config::validate` rejects others. The checker never
spawns subprocesses to learn installed versions — callers pipe the
version string into `check_installed`.
