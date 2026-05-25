---
id: extensibility
applies_to: [code]
anchors:
  - constitution
---

# Extensibility

Will the next change to this area be cheap or expensive?

- A new variant can be added without editing existing branches (the change
  doesn't bake in a closed assumption that the domain is open).
- Module boundaries expose interfaces/protocols, not concrete types.
- No premature abstraction either — flag speculative generality that adds
  indirection without a second caller.
