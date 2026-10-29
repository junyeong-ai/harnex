---
paths:
  - "crates/harness-cli/src/**"
  - "crates/harness-core/src/envelope.rs"
governs:
  concept: the JSON envelope contract every command emits
  live_truth:
    - crates/harness-core/src/envelope.rs
    - crates/harness-cli/src
---

# JSON envelope

Every CLI command emits exactly one envelope on stdout, terminated by `\n`.

Success: `{"ok": true, "data": T, "warnings": [...]}`
Error:   `{"ok": false, "error": {"code", "message", "hint?", "location?"}}`
List:    `data = {"items": [...], "total": N}`

Construct via `envelope::write_success(out, data, warnings)` or
`envelope::write_error(out, &error)`. Never write prose to stdout.
Stderr carries no contract but one line: `ask serve` writes `serving <url> for
<n> minutes` the moment it serves, since its envelope arrives only when the
wait ends and a `--no-open` caller reads the address there;
`crates/harness-cli/tests/ask.rs` holds its shape. The `guard` hook commands
also write to stderr, where the runtime reads a hook's notices
(`.claude/rules/guard.md`).

Severity enum (kebab-case in JSON): `blocker | major | minor | info`.

Slug grammar: kebab-case, greppable from the rule that produces it.

## Sanctioned exceptions to "one envelope on stdout"

These are the ONLY commands that emit non-envelope stdout, each opt-in and
documented — do not flag them as contract violations, and do not add new ones
without extending this list:

- `export schema <t> --raw` and `completions <shell> --raw`: emit the bare
  artifact (schema JSON / shell script) for committing to disk or sourcing.
  The default (no `--raw`) wraps in an envelope; `--raw` is the explicit
  opt-out for non-programmatic consumers.
- `guard floor`: speaks the PreToolUse hook contract instead of the envelope —
  a Block exits 2 with its reason on stderr (the documented feedback channel),
  a skip or grant emits one `{systemMessage, suppressOutput}` object, and a
  plain allow emits nothing. See guard.md.
- `guard telemetry-emit`: emits nothing on stdout and always exits 0 — it
  records a harness-invocation event as a side effect and must never block or
  interrupt the tool call that triggered it. See guard.md.
- A `guard` invocation that does not parse: one `{systemMessage,
  suppressOutput}` skip notice and exit 0, never the exit-2 error envelope —
  the runtime reads a hook's exit 2 as a Block, so an arm and a binary that
  disagree would refuse every tool call or keep a session from stopping.
