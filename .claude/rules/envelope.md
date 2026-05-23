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
