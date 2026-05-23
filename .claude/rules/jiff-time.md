---
paths:
  - "crates/harness-core/src/**"
---

# Time handling: jiff only

This project uses `jiff` (not `chrono`, not `time`). Rationale:
`jiff` follows Temporal-style API design, handles civil dates and
zoned timestamps as distinct types, and has correct DST/leap behavior
out of the box.

- Current UTC instant: `jiff::Timestamp::now()`
- Parse `YYYY-MM-DD`: `jiff::civil::Date::strptime("%Y-%m-%d", s)`
- Today (UTC): `Timestamp::now().to_zoned(TimeZone::UTC).date()`
- Duration arithmetic on Timestamps:
  `let elapsed: SignedDuration = a.duration_since(b);` then `.as_secs()`
- Calendar arithmetic on Dates:
  `let span: Span = d1.until((Unit::Day, d2))?;` then `.get_days()`

Never reintroduce `chrono` or `time` crate. Never use `SystemTime` for
arithmetic — convert to `Timestamp` first via `Timestamp::try_from`.
