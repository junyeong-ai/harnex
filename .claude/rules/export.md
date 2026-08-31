---
paths:
  - "crates/harness-core/src/export.rs"
  - "crates/harness-core/src/envelope.rs"
  - "crates/harness-core/src/config/**"
governs:
  concept: JSON Schema emission from the toolkit types
  live_truth:
    - crates/harness-core/src/export.rs
    - schemas/harness.schema.json
---

# export — JSON Schema emission

`schema_for(SchemaTarget)` emits draft-2020-12 JSON Schema for the
toolkit's user-facing types. Powered by `schemars` with the `jiff02` +
`semver1` feature flags so `jiff::Timestamp` and
`semver::Version` round-trip with correct schemas.

When adding a new schema target:
1. Add a `JsonSchema` derive to the public type (or define a shape struct
   in `envelope.rs` for envelope-like contracts).
2. Add one `Variant => "wire-string"` line to the `SchemaTarget` wire_enum
   (`ALL` and `from_str` are generated) + a match arm in `schema_for` —
   the exhaustive match forces it.
3. Raise the floor in `target_all_is_unique_and_nonshrinking` to the new
   `ALL` length, and name the target in README's `export schema` brace
   block — `the_readme_names_every_schema_the_binary_will_emit`
   (release_install_sync) fails until it appears.

`error-codes` derives from `ErrorCode::ALL` (the single source) via
`error_code_strings()` — no parallel hand-maintained list. The exhaustive
`ErrorCode::as_str` match forces `ALL` to stay complete; the
`error_code_tests` (in `error.rs`) and `error_codes_schema_lists_all_variants`
(in `export.rs`) catch drift.

`all` bundles every other target, derived from `SchemaTarget::ALL`; the
`all_schemas_emits_every_named_target` test iterates `ALL` too, so a new
target is covered without touching either.
