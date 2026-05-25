---
id: logic
applies_to: [code]
anchors:
  - constitution
---

# Logic

- No off-by-one errors in loops, slices, or boundary conditions.
- Concurrent access is guarded where shared state is mutated.
- Null/None/undefined paths are handled or documented as preconditions.
- State transitions are valid — no impossible states reachable.
- Arithmetic doesn't overflow, divide by zero, or lose precision silently.
