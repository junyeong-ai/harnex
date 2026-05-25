---
paths:
  - "**/*.py"
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.rs"
---

# Deprecation discipline

Deprecated code must carry a machine-readable marker with a sunset date.
Code without a marker is assumed active. The default policy is
delete-in-same-PR — a deprecation that lingers is technical debt.

## Allow marker format

```
# allow:deprecated <YYYY-MM-DD> <rationale>
```

- The marker sits on the line above or beside the deprecated symbol.
- `<YYYY-MM-DD>` is the sunset date — after this date, the marker
  becomes a finding on the next review.
- `<rationale>` is a one-line reason (migration path, blocking dependency).

## Policy

- **Default**: delete the deprecated code in the same PR that introduces
  the replacement. The allow marker is for cases where deletion is
  blocked by an external dependency or phased rollout.
- **Sunset enforcement**: an expired marker (date < today) surfaces as a
  Major finding during review. No auto-delete — operator confirms.
- **No zombie deprecations**: a marker without a date is invalid. Every
  deprecation must have a planned end.

<!-- Customize: adjust the marker format, severity, and default policy
     to match your team's tolerance for deprecated code. -->
