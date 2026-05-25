---
id: best-practice
applies_to: [code]
anchors:
  - constitution
---

# Best practice

Does the change honor the project's own architecture rules?

- Walk the change against each rule in `.claude/rules/` that its paths match.
- Flag a violation only by citing the specific rule it breaks — no finding
  without an anchor.
- Reuses an existing abstraction rather than reinventing one.
