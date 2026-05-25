---
id: completeness
applies_to: [code, spec, plan]
anchors:
  - constitution
---

# Completeness

- Every requirement in the spec has a corresponding implementation.
- Error paths are handled — not just the happy path.
- Edge cases identified in the plan are tested or explicitly deferred.
- Public API surfaces have documentation (doc-comments or README).
- Missing imports, unused variables, and dead code are flagged.
