---
paths:
  - "specs/**"
---

# Spec-driven workflow

Non-trivial features go through a spec before implementation. A spec is
a directory under `specs/<slug>/` with a lifecycle state tracked in
frontmatter. Small fixes (single-package, few files) commit directly.

## Decision threshold

- **Spec required**: crosses package boundaries, introduces a new
  abstraction, changes a public contract, or affects more than one team.
- **Direct commit**: bug fix, typo, single-package refactor within
  existing abstractions.

## Spec directory layout

```
specs/<slug>/
├── spec.md          # Problem + constraints + decision
├── plan.md          # Implementation plan (tasks, ordering, risks)
└── wrapup.md        # Post-implementation observations + learnings
```

## Lifecycle states

Tracked in `spec.md` frontmatter `status:` field.

| State | Meaning |
|---|---|
| `active` | Work in progress |
| `completed` | Successfully implemented; spec dir may be archived |
| `abandoned` | Decided not to proceed; rationale in spec.md |
| `superseded` | Replaced by a newer spec; link in frontmatter |

## Gates

Gates are decision points where progress pauses for review:

- **Scope gate**: before plan.md — is the problem well-defined?
- **Review gate**: before implementation — is the plan sound?
- **Wrapup gate**: after implementation — capture learnings.

<!-- Customize: add or remove phases and gates to match your team's
     development process. The structure is the value, not the exact
     phase count. -->
