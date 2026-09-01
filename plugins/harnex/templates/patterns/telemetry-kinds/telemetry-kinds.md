---
paths:
  - "harness.toml"
governs:
  concept: the closed-schema telemetry ledger and what may cross into it
  live_truth:
    - harness.toml
---

# Telemetry — a closed-Kind ledger the harness measures itself with

Every event the harness records is a declared **Kind** in
`[[telemetry.kinds]]` with a closed `payload_schema`. `harnex telemetry
append` validates each event against its schema at write time and refuses
anything outside it; `harnex telemetry report` rolls activity into trailing
windows, and the retirement sweep reads that rollup to find surfaces nothing
invokes.

## The contract

- **The schema is the privacy boundary, enforced.** A Kind's
  `payload_schema` lists exactly the fields that may cross — for the
  auto-emit Kind, the surface identifier and an outcome, never a tool's
  arguments, a file's contents, or anything a person typed. An undeclared
  field is rejected at append, so redaction is a schema fact rather than a
  reviewer's vigilance.
- **Outcome comes from the event, not the payload's word for it.** The
  auto-emit hook fires on both `PostToolUse` and `PostToolUseFailure`; which
  one fired is the outcome. A hook cannot mislabel a failure as a success
  because it never chooses the label.
- **Install-to-enable, and silent without the oracle.** The emit hook is a
  no-op when `harnex` is not on the path, and never blocks a tool call —
  telemetry that fails loud on absence would trade a measurement for an
  interruption.
- **A new Kind is a config edit, never a code edit.** Declare it in
  `[[telemetry.kinds]]`; the schema validator already knows how to enforce
  the closed shape.
- **The ledger is the retirement input.** A skill or agent whose identifier
  the ledger has not seen in the retirement window is a Silent candidate —
  which is why the surface identifier is the one field the auto-emit Kind
  carries.
