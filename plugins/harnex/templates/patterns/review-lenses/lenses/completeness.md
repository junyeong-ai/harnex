---
id: completeness
applies_to: [code, prose, spec, plan]
anchors:
  - rule:constitution
---

# Completeness

Does the change address the WHOLE requirement, not just the demonstrated path?

- Error and failure paths are handled, not only the happy path.
- Edge cases named in the spec are tested or explicitly deferred with reason.
- A new public surface has the contract documented where consumers look.
- Each new test fails when its subject is broken. One seeded to the degenerate
  state — an empty fixture, a stub standing in for the thing under test, an
  assertion the subject cannot violate — certifies green over a subject that
  does nothing, and the suite reports it as covered. Break the subject once and
  watch the test go red.

(Linter-owned checks — unused imports, dead code — are out of scope; the
formatter and type checker own them.)
