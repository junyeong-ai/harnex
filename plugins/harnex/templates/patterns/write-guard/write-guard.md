---
paths:
  - "./hooks/check-pre-write.*"
  - ".claude/settings.json"
---

# Write guard — PreToolUse enforcement

`hooks/check-pre-write.sh` runs on every Edit/Write tool call BEFORE the
write completes. Exit 0 allows the write; exit 2 blocks it with a reason
fed back to Claude via stderr. This is the strongest domain enforcement
surface — `permissions.deny` controls WHICH tools run; the write guard
controls WHAT CONTENT reaches disk.

## Exit code contract

| Code | Meaning | When to use |
|------|---------|-------------|
| `0` | Allow the write | Default; every check passed |
| `2` | Block the write | Convention violation; plain-text reason on stderr |
| `1` | Non-blocking error | Hook malfunction; write proceeds |

Exit 2 is the ONLY blocking code. stderr text feeds back to Claude as
context explaining why the write was blocked. A check that intends
"found something interesting but don't block" must exit 0 with a
`systemMessage` in stdout JSON, never exit 1.

## Adding a convention check

Add a case arm or function in `hooks/check-pre-write.sh`. Each check:
1. Inspects the target file path (and optionally the proposed content).
2. On violation: prints a plain-text reason to stderr, then exits 2.
3. On pass: falls through to the next check.

The verifier runs BEFORE the write — `$FILE` contains the path to the
CURRENT file on disk, not the proposed edit. To inspect the proposed
content, parse `tool_input` from the captured stdin variable: Write
provides `tool_input.content`; Edit provides `tool_input.old_string`
and `tool_input.new_string`.

## Stdin capture

The template captures stdin into `$INPUT` at the top of the script.
All subsequent `jq` reads must pipe from `$INPUT`, not read stdin
directly (stdin is consumed by the first read).

## Fail-open discipline

The verifier skips (exit 0) when jq is absent, the file path is empty,
or path traversal is detected. A broken check must never trap the
developer in a write-blocked state — fix the check, not the escape.
