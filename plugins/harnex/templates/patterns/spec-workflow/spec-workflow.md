---
paths:
  - "specs/**"
governs:
  concept: the spec lifecycle and its gates
  live_truth:
    - specs
    - .claude/skills/spec
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

Read the artifacts that way. `spec.md`'s acceptance criteria are what
wrapup answers against, not a briefing. `plan.md`'s Outstanding issues
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

## Spec directory layout

```
specs/
├── _template/       # copy this directory to start a spec
│   ├── spec.md
│   ├── plan.md
│   └── wrapup.md
└── <slug>/
    ├── spec.md      # REQUIRED — problem + constraints + acceptance criteria
    ├── plan.md      # decisions + touched files + task list + Outstanding issues
    └── wrapup.md    # REQUIRED at the end — what the criteria got, what is left
```

Start a spec by copying `specs/_template/` to `specs/<slug>/` and filling the
`<...>` placeholders. The template carries one file per artifact-producing
phase, so the file set IS the pipeline — dropping a phase drops its template in
the same commit.

`plan.md` is optional for a change small enough that its decisions fit in the
spec, and every gate then reports to the conversation instead of to
`## Outstanding issues`. Take that branch deliberately: a spec with no plan has
nowhere to leave a finding for the next session.

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
| `active` | Work in progress. The directory is live state. |
| `completed` | Implemented, wrapup captured, **directory retired** |
| `abandoned` | Decided not to proceed, **directory retired** |
| `superseded` | Replaced by another spec; the directory stays, because it is now a pointer and a pointer with no target is worse than the file |

`completed` and `abandoned` both retire the directory: the durable half moves
to wherever this project keeps learnings, references retarget onto it, and the
spec directory is removed. The two differ only in what the record says. A spec
directory is state for work in flight, and `.claude/rules/artifact-lifecycle.md`
retires every other artifact that stopped earning its context cost — a finished
spec is not an exception to that, it is the clearest case of it.

## Where the procedure lives

Running a spec is a workflow, so it is a skill: `.claude/skills/spec/`. The
orchestrator derives the phase, fires five gate events, and records each
decision. This file is the part that is guidance rather than procedure — when
to reach for a spec at all, what the directory holds, and what the lifecycle
words mean.

Phase is **not** stored in frontmatter. The skill derives it, so a fresh session
re-reads it from the repository and there is no second copy to go stale; the
table that does the reading is in `.claude/skills/spec/SKILL.md`. Findings land
in `plan.md ## Outstanding issues`, which is what makes the next session read
what the last one found.

## Enforcement

`hooks/pre-commit.d/check-plan.sh` runs `harnex plan audit` over every staged
plan: an open Critical/Blocker row, a row deleted, reworded or downgraded
instead of gaining its terminal disposition, a decision log whose
Critical+Blocker count will not fall, and a committed decision bullet edited
instead of appended each block the commit — the grammar the gates write is
computed there, never trusted to narration. For a gate before the session ends
rather than at the commit, `harnex guard stop-audit` (`[guard.stop_audit]` in
`harness.toml`) adds a bounded fresh-context critique at Stop.
