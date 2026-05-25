---
id: extensibility
applies_to: [code]
anchors:
  - constitution
---

# Extensibility

- New variants (types, handlers, strategies) can be added without
  modifying existing code (open-closed).
- Abstractions use interfaces/protocols, not concrete classes, at
  module boundaries.
- Hardcoded values that might change are extracted to configuration.
- The change doesn't introduce a pattern that makes future changes
  disproportionately expensive.
