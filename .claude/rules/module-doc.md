---
paths:
  - "crates/harness-core/src/**/mod.rs"
governs:
  concept: the module doc contract
  live_truth:
    - crates/harness-core/src/audit
    - crates/harness-core/src/codegen
    - crates/harness-core/src/config
    - crates/harness-core/src/evidence
    - crates/harness-core/src/graph
    - crates/harness-core/src/guard
    - crates/harness-core/src/lifecycle
    - crates/harness-core/src/policy
    - crates/harness-core/src/session
    - crates/harness-core/src/telemetry
    - crates/harness-core/src/validate
---

# Module documentation

Every `mod.rs` ships a `//!` doc block with (`lib.rs` is exempt — it only
declares modules, with no behavior to document):

1. **WHAT** — one-sentence module purpose.
2. **HOW** — key types and their responsibility (concise).
3. **WHAT THIS MODULE REFUSES TO DO** — explicit negative space.

The negative-space section is non-optional. It defines the module's
contract by what it deliberately excludes.
