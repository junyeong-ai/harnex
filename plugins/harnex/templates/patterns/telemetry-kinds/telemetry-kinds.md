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
- Set `invocation_kind = "harness_invocation"` on the `[[kinds]]` this record
  can name — the skill and sub-agent kinds, whose slugs are what `surface`
  carries. Never on a rule kind or any artifact that is loaded rather than
  invoked: it can never appear here, so reading its absence would retire every
  one of them the moment a single skill runs. A kind left undeclared stays
  `unmeasured`, which is the honest answer for it.
- Read the ledger for retirement with `harnex lifecycle retire`: within a
  window this Kind recorded, a surface it never names is Silent — the slug is
  why the Kind carries it. A window it did not record is Unmeasured, never a
  fabricated Silent, so the signal means something only once the emit has been
  filling the ledger. (`harnex telemetry report` rolls counts by Kind, not by
  surface, so it does not answer the per-surface question.)
