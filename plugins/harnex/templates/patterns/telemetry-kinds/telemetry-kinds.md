---
paths:
  - "harness.toml"
governs:
  concept: the closed-schema telemetry ledger and what may cross into it
  live_truth:
    - harness.toml
---

# Telemetry — a closed-Kind ledger

Declare every event as a Kind in `[[telemetry.kinds]]` with a closed
`payload_schema`. Emit with `harnex telemetry append`; it rejects any field
outside the schema at write time.

- Cross only what the schema declares. The auto-emit Kind carries the invoked
  element's slug and an outcome — never a tool's arguments, a file's contents,
  or anything a person typed. An undeclared field fails the append.
- Let the event decide the outcome. `harnex guard telemetry-emit` reads which
  hook fired — `PostToolUse` or `PostToolUseFailure` — never a payload field;
  a failure cannot be recorded as a success.
- Wire the emit to `PostToolUse` and `PostToolUseFailure`, matcher
  `Skill|Task|Agent`, best `async`. It is a no-op without the oracle and never
  blocks a tool call.
- Add a Kind with a config edit, never a code edit.
- Read the ledger for retirement with `harnex lifecycle retire`: it scans raw
  payloads for a surface's slug within the silence window. A surface the ledger
  has not seen is Silent — which is why the slug is the field the auto-emit
  Kind carries. (`harnex telemetry report` rolls counts by Kind, not by
  surface, so it does not answer the per-surface question.)
