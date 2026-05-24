---
paths:
  - "crates/harness-core/src/**/mod.rs"
  - "crates/harness-core/src/lib.rs"
---

# Module documentation

Every `mod.rs` and `lib.rs` ships a `//!` doc block with:

1. **WHAT** — one-sentence module purpose.
2. **HOW** — key types and their responsibility (concise).
3. **WHAT THIS MODULE REFUSES TO DO** — explicit negative space.

The negative-space section is non-optional. It defines the module's
contract by what it deliberately excludes.
