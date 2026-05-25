---
paths:
  - "crates/harness-core/src/**/mod.rs"
---

# Module documentation

Every `mod.rs` ships a `//!` doc block with (`lib.rs` is exempt — it only
declares modules, with no behavior to document):

1. **WHAT** — one-sentence module purpose.
2. **HOW** — key types and their responsibility (concise).
3. **WHAT THIS MODULE REFUSES TO DO** — explicit negative space.

The negative-space section is non-optional. It defines the module's
contract by what it deliberately excludes.
