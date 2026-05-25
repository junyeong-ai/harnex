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

## Phases (default 5-phase pipeline)

Each phase produces an artifact and passes through a gate before the
next phase begins. Add or remove phases to match your team's process.

| Phase | Artifact | Gate | Done when |
|---|---|---|---|
| **specify** | `specs/<slug>/spec.md` | Scope gate — is the problem well-defined? Constraints clear? | Problem statement + acceptance criteria reviewed |
| **plan** | `specs/<slug>/plan.md` | Review gate — is the solution sound? Risks identified? | Implementation plan + task decomposition approved |
| **implement** | source code | — (continuous) | All planned tasks completed; tests pass |
| **validate** | test results, review | Validation gate — does it meet acceptance criteria? | Review lenses pass; acceptance criteria verified |
| **wrapup** | `specs/<slug>/wrapup.md` | — | Learnings captured; spec status → completed |

### Optional phases (web/app projects)

Insert these between implement and validate, or between validate and
wrapup, as the project requires:

- **preview**: visual or interactive verification before formal validation.
  Useful for UI-heavy projects with design review cycles.
- **deploy**: production deployment + rollback verification. Useful for
  projects with explicit deploy gates (staging → production).

## Spec directory layout

```
specs/<slug>/
├── spec.md          # Problem + constraints + acceptance criteria
├── plan.md          # Solution design + tasks + risks
├── wrapup.md        # Post-implementation observations + learnings
└── learning.md      # (optional) Promoted patterns from this spec
```

## `spec.md` frontmatter

```yaml
---
slug: <kebab-case-identifier>
title: <human-readable title>
status: active          # active | completed | abandoned | superseded
created: <YYYY-MM-DD>
superseded_by:          # slug of replacement spec, if superseded
---
```

## Lifecycle states

| State | Meaning |
|---|---|
| `active` | Work in progress |
| `completed` | Implemented; wrapup captured |
| `abandoned` | Decided not to proceed; rationale in spec.md |
| `superseded` | Replaced by another spec; link in frontmatter |

## Gates

Gates are decision points where progress pauses for verification:

- **Scope gate** (before plan): Is the problem well-defined? Are
  constraints clear? Does it need a spec at all (direct-commit check)?
- **Review gate** (before implement): Is the plan sound? Are risks
  identified? Is the decomposition testable?
- **Validation gate** (before wrapup): Do the results meet the
  acceptance criteria from spec.md? Do review lenses pass?

A gate failure sends work back to the previous phase with specific
feedback — never forward past a failed gate.

## Resume semantics

A spec can be resumed from any phase. The resume command detects the
current phase from which artifacts exist and which are missing:
- spec.md exists, plan.md missing → resume at plan
- plan.md exists, code not complete → resume at implement
- Implementation complete, wrapup.md missing → resume at wrapup
