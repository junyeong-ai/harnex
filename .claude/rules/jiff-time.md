---
paths:
  - "crates/harness-core/src/**"
---

# Time handling: jiff only

Use `jiff` for all time operations. Never `chrono`, `time`, or raw
`SystemTime` arithmetic.

- Current UTC instant: `jiff::Timestamp::now()`
- Parse `YYYY-MM-DD`: `jiff::civil::Date::strptime("%Y-%m-%d", s)`
- Today (UTC): `Timestamp::now().to_zoned(TimeZone::UTC).date()`
- Duration arithmetic on Timestamps:
  `let elapsed: SignedDuration = a.duration_since(b);` then `.as_secs()`
- Calendar arithmetic on Dates:
  `let span: Span = d1.until((Unit::Day, d2))?;` then `.get_days()`

Convert `SystemTime` to `Timestamp` via `Timestamp::try_from` before
arithmetic.
