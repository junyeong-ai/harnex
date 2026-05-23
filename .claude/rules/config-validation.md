---
paths:
  - "crates/harness-core/src/config/**"
---

# Config validation

Every load-time check rejects configurations the runtime cannot honor:

- Names declared in one section resolve to names declared in another
  (e.g., `default_provenance` matches a registered verifier).
- Glob patterns compile.
- Enum-valued strings are in the closed set.
- Reference paths exist relative to the config file.
- Numeric thresholds are in valid ranges.

When adding a new config section:
1. Add the field to `Config` and its substruct.
2. Add a validation arm to `Config::validate`.
3. Add a unit test in `config/mod.rs` that constructs an invalid config
   and asserts the matching `ErrorCode`.
