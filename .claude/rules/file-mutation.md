---
paths:
  - "crates/harness-core/src/**"
---

# File mutation

Every write to a project file routes through one of two
`harness_core::path_guard` functions:

- `write_atomic` — full-file replace via same-directory temp file + rename.
- `append_line` — append-only ledgers (observation, decision JSONL).

Direct `std::fs::write`, `File::create + write_all`, `OpenOptions::append`,
or any other write primitive is forbidden in domain modules.

Both functions enforce:
- `reject_traversal`: rejects any path containing `..` segments.
- `reject_symlink_write`: refuses to overwrite a symlink target.
- Parent directory creation as needed.

Exception: `telemetry::JsonlStorage::append` calls `reject_traversal`
and `reject_symlink_write` directly because its advisory-lock +
rotation critical section requires holding the file handle open across
operations, which is incompatible with `append_line`'s self-contained
open-write-close cycle. The safety guards are still applied.

Reads MAY follow symlinks (scanner indexes linked content). Only writes
refuse symlinks.
