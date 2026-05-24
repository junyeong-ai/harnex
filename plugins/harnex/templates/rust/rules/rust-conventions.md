---
paths:
  - "**/*.rs"
---

# Rust conventions

Project-specific decisions that rustfmt and clippy do not enforce.
Style lives in rustfmt — never restate here.

## Module shape

- Every `mod.rs` ships a `//!` doc block with WHAT + WHAT-REFUSED.
  The negative-space section is non-optional; without it future
  contributors fill the vacuum with scope creep.

## Errors

- Every failure is a `thiserror::Error` variant with a stable code.
  No `String` errors. No `Box<dyn Error>` at module boundaries.
- IO failures carry the exact path that triggered them.

## Closed-set enums

- Vocabularies (strategies, kinds, formats, decisions) are typed enums
  with `const ALL`, `from_str`, `as_str`. The exhaustive match enforces
  every consuming site updates at compile time. No parallel `KNOWN_*`
  const — drift is forbidden by Constitution IX.

## Concurrency

- No async runtime by default. Reach for `rayon` when CPU-parallel work
  emerges; never `tokio` without a recorded decision in the lifecycle
  ledger.
