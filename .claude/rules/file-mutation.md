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
- `reject_symlink_write`: refuses to overwrite a symlinked **leaf**.
- Parent directory creation as needed.

`reject_symlink_write` guards the leaf only — symlinked ancestor
directories are out of scope. Covering them soundly needs a trusted-root
anchor (a root-down walk false-rejects legitimate paths under system mount
symlinks like macOS `/var -> /private/var`; an immediate-parent check both
false-rejects those and misses the realistic two-level `.harness -> /out`
case). This is a deliberate threat-model boundary: `harness` is a local,
single-user, no-network CLI on the user's own repo — an attacker who can
plant a symlink in the working tree already controls it. The in-contract
guarantees are leaf-overwrite refusal + `..` rejection. Any path whose
filename is derived from input must additionally be pinned to a single path
component by the caller (telemetry `append` rejects a kind whose
`{kind}.jsonl` is not a plain filename — separators, absolute, `.`/`..`).

Exception: `telemetry::JsonlStorage::append` applies the guards directly
(not via `append_line`) because its advisory-lock + rotation critical
section holds the file handle open across operations, incompatible with
`append_line`'s self-contained open-write-close cycle.

Reads MAY follow symlinks (scanner indexes linked content). Only writes
refuse symlinks.
