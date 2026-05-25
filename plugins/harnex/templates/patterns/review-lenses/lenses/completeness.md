---
id: completeness
applies_to: [code, spec, plan]
anchors:
  - constitution
---

# Completeness

Does the change address the WHOLE requirement, not just the demonstrated path?

- Error and failure paths are handled, not only the happy path.
- Edge cases named in the spec are tested or explicitly deferred with reason.
- A new public surface has the contract documented where consumers look.

(Linter-owned checks — unused imports, dead code — are out of scope; the
formatter and type checker own them.)
