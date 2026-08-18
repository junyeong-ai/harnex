# Gate events

Four events. Each ends in a decision, and the decision is recorded before the
work moves — an unrecorded gate is a gate that did not fire.

## The decision token

A closed set of four. Nothing else is a decision:

| Token | Means |
|---|---|
| `approved` | proceed |
| `rejected` | this is not the change to make |
| `needs_revision` | go back with named findings |
| `deferred` | not now, and the record says why |

Everything else — what was found, what was weighed, what the user said — goes
in the rationale beside the token. Widening the enum is how a decision log
stops being queryable.

## Recording

Append one bullet per firing to `## Decision Log` in `specs/<slug>/spec.md`:

```
- 2026-01-15 · review · needs_revision · two Blockers in the migration path; see plan.md ## Outstanding Issues
```

Append-only. A gate that fires three times leaves three bullets in order — the
history of a decision is the interesting part, and overwriting keeps only the
last one. `git log specs/<slug>/` is the timeline this rides on.

## clarify — inline, during specify

Fires when the ambiguity scan finds a question that changes what gets built.
Ask it, take the answer, record `approved` with what was answered and what was
deferred. Resolves in the conversation; no round trip.

Questions the codebase answers are not clarify questions. Answer them.

## design_review — conditional, blocking, end of plan

Fires when the plan crosses a blast-radius signal this project declares. A plan
below every signal skips it — the gate exists for decisions that are expensive
to reverse, and firing it on all of them teaches everyone to click through.

Spawn a reviewer with fresh context over `plan.md`'s decisions. Tell it to
refute, not to approve. On a Critical or Blocker: record `needs_revision`,
revise, re-fire. On a clean report: transcribe what remains into
`## Outstanding Issues`, record `approved`, proceed.

Judge the round on the verdict the reviewer delivered, never on the spawn
having finished. A reviewer that returned nothing has produced no report;
re-fire rather than approve.

## review — blocking, end of implement

Delegate to the review loop (`.claude/skills/review/`) over the spec's diff.
The loop converges or reports why it stopped.

Zero Critical and zero Blocker passes the gate. Write what remains to
`plan.md ## Outstanding Issues` — Major and Minor are follow-up signal, not
blockers — and record `approved`. Otherwise record `needs_revision` with the
count, and the work goes back.

## resume — inline, on a dirty worktree

Fires when a resume finds uncommitted changes: the previous session left work
in flight and what it intended is not on disk. Show what changed, ask whether
to keep going, land it first, or discard, and record the answer. Then follow
[resume.md](resume.md).

A clean worktree resumes without this gate.
