---
paths:
  - "crates/harness-core/src/telemetry/**"
governs:
  concept: the append-only telemetry ledger
  live_truth:
    - crates/harness-core/src/telemetry
---

# Telemetry closed schema

Every telemetry Kind declares a `payload_schema` (closed object) in
`harness.toml`. Append-time validation rejects:
- Missing required fields.
- Fields not declared in `properties`.
- Type mismatches (string / integer / number / boolean).
- Enum value mismatches.

Schema extension requires a config edit, not a code edit. Adding a new
Kind never requires touching `KindSchema`.

`StorageKind` is the storage strategy enum (its variants are the source of
truth — `StorageKind::ALL`). `JsonlStorage` rotates files at the configured
byte size and never silently deletes.

`TelemetryQuery::report(windows, kind_filter)` returns a per-Kind rollup:
total + first/last seen + counts within each trailing-day window. Used
by `harnex telemetry report`. Retirement's Silent signal is a separate,
per-slug computation over `scan_events`, not this rollup — see `lifecycle.md`
for how a slug's silence state is derived. Window
arithmetic uses `jiff::SignedDuration::from_hours(days * 24)` because
`jiff::Timestamp` arithmetic forbids calendar-unit spans.
