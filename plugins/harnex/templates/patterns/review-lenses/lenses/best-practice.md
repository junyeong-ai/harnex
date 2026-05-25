---
id: best-practice
applies_to: [code]
anchors:
  - constitution
---

# Best practice

- Follows the architecture rules in `.claude/rules/`.
- Uses established abstractions — no reinvention of existing utilities.
- Dependencies flow in the declared direction (no circular imports).
- Configuration is explicit (no magic defaults hidden in code).
- Side effects are at the boundary, not deep in pure logic.
