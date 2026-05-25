---
id: logic
applies_to: [code]
anchors:
  - constitution
---

# Logic

Is the behavior correct on the paths tests did not exercise?

- Concurrent access to shared mutable state is guarded.
- State machines reject impossible transitions rather than silently accepting.
- Boundary inputs (empty, max, absent) reach a defined outcome.

(Focus on what testing missed, not a generic bug checklist — the model and
the test suite catch the obvious cases.)
