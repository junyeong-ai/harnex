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

- <!-- harnex-fill: error type in use — `thiserror`, `anyhow`, `snafu`, custom -->
- Common default: a typed error enum per module boundary; IO failures carry
  the path that triggered them. Replace if the project uses `anyhow` end to
  end.

## Module shape

- <!-- harnex-fill: doc-comment discipline in existing `mod.rs` files, if any -->
- Common default: each `mod.rs` ships a `//!` doc block stating purpose and
  what the module deliberately excludes.

## Concurrency

- <!-- harnex-fill: async runtime in `Cargo.toml` — tokio, async-std, smol, none -->
- Common default: prefer `rayon` for CPU-parallel work; introduce an async
  runtime only when IO concurrency genuinely requires it. Replace with the
  project's actual runtime if one is already chosen.

<!-- Replace every `harnex-fill` marker with what this codebase actually
     does AND where that is declared, as a backtick path — `ruff` —
     `pyproject.toml`. A convention with no named owner is prose that drifts
     the day someone changes the tool, and nothing catches it; a pointer can be
     checked by a reader, and `harness check` resolves a marked claim —
     `[file: path/to/thing.py:42]`, the line optional — against the tree. Both harnesses this template is modelled on name an
     owner in every rule they carry.

     A section with no signal yet takes an explicit "none observed yet" plus
     the default that applies until one appears. -->
