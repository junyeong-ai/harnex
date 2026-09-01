---
paths:
  - "crates/harness-core/src/**"
governs:
  concept: the single safe write module
  live_truth:
    - crates/harness-core/src/path_guard.rs
---

# File mutation

Every write to a project file routes through one of two
`harness_core::path_guard` functions:

- `write_atomic` ([file: crates/harness-core/src/path_guard.rs:96]) — full-file
  replace via same-directory temp file + rename.
- `append_line` ([file: crates/harness-core/src/path_guard.rs:133]) — append-only
  ledgers (observation, decision JSONL).

Direct `std::fs::write`, `File::create + write_all`, `OpenOptions::append`,
or any other write primitive is forbidden in domain modules.

Both functions enforce:
- `reject_traversal`: rejects any path containing `..` segments.
- `reject_symlink_write`: refuses to overwrite a symlinked **leaf**.
- Parent directory creation as needed.

`reject_symlink_write` guards the leaf only (ancestor symlinks are out of
scope — see `path_guard.rs` for the threat-model reasoning). Input-derived
filenames must be pinned to a single component by the caller (telemetry
`append` rejects a kind whose `{kind}.jsonl` escapes the dir).

Exception: `telemetry::JsonlStorage::append` applies the guards directly
(not via `append_line`) because its advisory-lock + rotation critical
section holds the file handle open across operations, incompatible with
`append_line`'s self-contained open-write-close cycle.

Reads MAY follow symlinks (scanner indexes linked content). Only writes
refuse symlinks.
