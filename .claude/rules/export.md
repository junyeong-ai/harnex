---
paths:
  - "crates/harness-core/src/export.rs"
  - "crates/harness-core/src/envelope.rs"
  - "crates/harness-core/src/config/**"
---

# export — JSON Schema emission

`schema_for(SchemaTarget)` emits draft-2020-12 JSON Schema for the
toolkit's user-facing types. Powered by `schemars` with the `jiff02` +
`semver1` feature flags so `jiff::Timestamp` and
`semver::Version` round-trip with correct schemas.

When adding a new schema target:
1. Add a `JsonSchema` derive to the public type (or define a shape struct
   in `envelope.rs` for envelope-like contracts).
2. Add a variant to `SchemaTarget` + a match arm in `schema_for` + a string
   entry in `SchemaTarget::from_str`.
3. Add a test under `export::tests` that asserts a structural property
   (e.g., expected key presence).

`error-codes` is hand-rolled — every `ErrorCode` variant must appear in
`error_code_strings()`. Test `error_codes_schema_lists_all_variants`
catches drift if a new variant is missed.

`all` bundles every other target. Adding a new target requires adding it
to `all_schemas` AND the `all_schemas_emits_every_named_target` test.
