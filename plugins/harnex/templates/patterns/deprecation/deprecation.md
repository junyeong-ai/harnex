---
paths:
  - "**/*.py"
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.rs"
---

# Deprecation discipline

Deprecated code must carry a machine-readable marker with a sunset date.
Code without a marker is assumed active. The right default depends on the
consumer surface: choose one below and delete the other.

## Allow marker format

```
# allow:deprecated <YYYY-MM-DD> <rationale>
```

- The marker sits on the line above or beside the deprecated symbol.
- `<YYYY-MM-DD>` is the sunset date — after this date, the marker
  becomes a finding on the next review.
- `<rationale>` is a one-line reason (migration path, blocking dependency).

## Policy (pick one — based on consumer surface)

- **Internal-only consumers** (all call sites in this repo): delete the
  deprecated code in the same PR that introduces the replacement. A version
  alias saves zero external-stability cost and is permanent overhead; the
  allow marker is only for a move blocked by an in-flight migration.
- **Public / external consumers** (published API, other teams): keep a
  staged sunset window. The marker's `<YYYY-MM-DD>` is the window end; ship
  the replacement, mark the old path, and remove it after the date once
  consumers have migrated.

Common to both:
- **Sunset enforcement**: an expired marker (date < today) surfaces as a
  Major finding during review. No auto-delete — operator confirms.
- **No zombie deprecations**: a marker without a date is invalid. Every
  deprecation has a planned end.

<!-- Customize: keep the regime matching your consumer surface; adjust the
     marker format and severity to your team's tolerance. -->
