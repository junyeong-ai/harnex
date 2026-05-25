---
paths:
  - "**/*.rs"
---

# Rust conventions

Project-specific decisions that rustfmt and clippy do not enforce. Style
lives in rustfmt — never restate here. Scaffold fills each section from the
codebase it observes; the entries below are common defaults to keep only if
they match the project's actual practice.

## Errors

- Observed: <error type in use — `thiserror`, `anyhow`, `snafu`, custom>.
- Common default: a typed error enum per module boundary; IO failures carry
  the path that triggered them. Replace if the project uses `anyhow` end to
  end.

## Module shape

- Observed: <doc-comment discipline in existing `mod.rs` files, if any>.
- Common default: each `mod.rs` ships a `//!` doc block stating purpose and
  what the module deliberately excludes.

## Concurrency

- Observed: <async runtime in `Cargo.toml` — tokio, async-std, smol, none>.
- Common default: prefer `rayon` for CPU-parallel work; introduce an async
  runtime only when IO concurrency genuinely requires it. Replace with the
  project's actual runtime if one is already chosen.

<!-- Scaffold: detect the real conventions from existing code and replace the
     "Observed:" lines. If a section has no signal yet, keep the default and
     note "none observed yet". -->
