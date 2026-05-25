---
paths:
  - "crates/harness-cli/src/**"
  - "crates/harness-core/src/envelope.rs"
---

# JSON envelope

Every CLI command emits exactly one envelope on stdout, terminated by `\n`.

Success: `{"ok": true, "data": T, "warnings": [...]}`
Error:   `{"ok": false, "error": {"code", "message", "hint?", "location?"}}`
List:    `data = {"items": [...], "total": N, "skipped_rules"?: [...]}`

Construct via `envelope::write_success(out, data, warnings)` or
`envelope::write_error(out, &error)`. Never write prose to stdout.
Stderr is reserved for debug logging; production builds emit nothing
to stderr.

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
- `guard hook-run`: propagates the wrapped hook's exit code verbatim (the
  code is meaningful to Claude Code — e.g. PreToolUse exit 2 blocks the
  action). See guard.md.
- `guard stop-audit`: maps a `Block` to exit 2 (Stop-hook force-continuation),
  the sole exception to Article II's exit-code set. See guard.md.
