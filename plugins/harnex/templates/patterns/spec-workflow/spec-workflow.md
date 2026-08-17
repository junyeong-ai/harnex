---
paths:
  - "specs/**"
---

# Spec-driven workflow

A spec is a directory under `specs/<slug>/` with a lifecycle state tracked
in frontmatter.

## What a spec is for

Two things, and neither is telling the model what to do:

1. **A checkpoint a person reviews before the work lands.** The shape gets
   approved while it is still cheap to change.
2. **The state file the work runs from.** Sessions compact; a conversation
   does not survive its own context window, and a file does. `agent-conduct.md`
   puts structured facts in a structured file — for a spec, this directory is
   that file.

Read the artifacts that way. `spec.md`'s acceptance criteria are what the
validation gate checks against, not a briefing. `plan.md`'s Outstanding Issues
is where the review gate writes, so the next session reads what the last one
found instead of rediscovering it.

## Decision threshold

Ask the two questions above, not how large the change is:

- **Spec**: someone other than the author needs to approve the shape before it
  lands, **or** the work will outlive one context window and its state has to
  live somewhere a fresh session can read.
- **Direct commit**: neither holds. It lands green in one pass and the commit
  body carries everything a later reader needs.

Size and file count are proxies for the second question, and they decay — what
took a week of sessions when this rubric was written may now be one. Ask the
question the artifact answers, not the proxy.

## Phases (default 5-phase pipeline)

Each phase produces an artifact and passes through a gate before the
next phase begins. Add or remove phases to match your team's process — a
phase whose artifact nobody reviews and no later session reads is ceremony,
and the two questions above are how to tell.

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
specs/
├── _template/       # copy this directory to start a spec
│   ├── spec.md
│   ├── plan.md
│   └── wrapup.md
└── <slug>/
    ├── spec.md      # Problem + constraints + acceptance criteria
    ├── plan.md      # Solution design + tasks + risks
    ├── wrapup.md    # Post-implementation observations + learnings
    └── learning.md  # (optional) Promoted patterns from this spec
```

Start a spec by copying `specs/_template/` to `specs/<slug>/` and filling the
`<...>` placeholders. `_template/` carries one file per phase that produces an
artifact, so the file set is the pipeline: a phase added or dropped above is
added or dropped there in the same commit.

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
