# Constitution

## I. Pure determinism

No async runtime. No network at command time. No AI / agent / server
dependencies. `rayon` is the only permitted concurrency primitive (and
not yet used).

## II. JSON envelope is the only output contract

Every CLI command emits exactly one `{ok, data?, error?, warnings[]}`
JSON object on stdout. Exit 0 = success, 1 = validation finding,
2 = runtime failure. No prose on stdout.

## III. Single safe write module

Every file mutation routes through `harness_core::path_guard` — either
`write_atomic` (full replace via temp + rename) or `append_line`
(append-only ledgers). Both enforce traversal rejection + symlink-write
rejection. CLI handlers MUST NOT call `std::fs::write` directly.

## IV. Config validates at load time

`Config::validate` rejects any configuration the runtime cannot honor.
Duplicate names, unknown strategies, unresolvable references, malformed
schemas — all fail at load, never silently at runtime.

## V. Closed schemas

Every typed surface (telemetry payload, frontmatter, finding) is a closed
schema. Fields outside the declared shape are rejected at the boundary,
never silently accepted.

## VI. Typed errors, stable codes

Every failure is a variant of `harness_core::error::Error` with a stable
`ErrorCode`. The string serialisation of `ErrorCode` is a public contract
and may only change with a major version bump.

## VII. No project vocabulary

Source code carries no project-specific names, slugs, or thresholds.
Every project-specific shape derives from `harness.toml`.

## VIII. No human-pedagogical prose in rule files

Rule files (`.claude/rules/*.md`) use imperatives. Background and
reasoning live in commit bodies and the lifecycle ledger, not in the
rule body that Claude re-reads every session.
