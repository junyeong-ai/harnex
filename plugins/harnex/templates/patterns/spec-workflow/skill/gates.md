# Gate events

Five events. Each ends in a decision, and the decision is recorded before the
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

Append one bullet per firing to `## Decision log` in `specs/<slug>/spec.md` —
the section `specs/_template/spec.md` ships for exactly this:

```
- 2026-01-15 · review · needs_revision · two Blockers in the migration path; see plan.md ## Outstanding issues
```

Append-only, and a merge is no exception: bringing a branch's bullets in ahead
of ones already committed rewrites the log, so merge by appending the other
side's records after yours. A gate that fires three times leaves three bullets
in order — the history of a decision is the interesting part, and overwriting
keeps only the last one. `git log specs/<slug>/` is the timeline this rides on.

No count of rounds bounds a gate: a review still finding new defects is doing
what it is for. The gate goes back to the operator on what its rounds find:

- **diverged** — a Critical or Blocker sits in what the previous revision
  wrote, and no smaller revision removes it (`design_review` below);
- **returned** — a Critical or Blocker of a class an earlier revision claimed
  to close comes back where that revision did not reach;
- **disputed** — two rounds in a row each file a Critical or Blocker, and every
  one is refuted against ground truth;
- **stalled** — a round's Critical and Blocker findings are ones an earlier
  round already filed.

The operator decides between one more revision, a split that reviews each part
as its own unit, disposing of every blocking row and approving, and stopping
the work. Each is recorded as the decision it is.

A row in `## Outstanding issues` is never deleted, settled as much as open:
the section is what review found, and a disposition ends a finding rather than
retiring its row.

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
refute, not to approve, and charge it with the document alone — never with
what earlier rounds filed or which revisions to look at, because a reviewer
steered by the last round's verdicts is that round again. Write what it found
to `## Outstanding issues`. On a Critical or Blocker: record `needs_revision`,
revise, re-fire. On a clean report: record `approved`, proceed.

Revise by the smallest change that removes the finding. A Critical or Blocker
that sits in what the previous revision wrote says that revision failed:
rework or remove it rather than adding a condition to it. Every condition
added to answer a round is new surface for the next round to refute, and a
loop that answers its findings that way diverges instead of converging on a
design.

Judge the round on the verdict the reviewer delivered, never on the spawn
having finished. A reviewer that returned nothing has produced no report;
re-fire rather than approve.

## review — blocking, end of implement

Delegate to <!-- harnex-fill: what this project reviews a diff with — the
`.claude/skills/review/` loop when the review-lenses pattern is installed,
otherwise the project's own review command or a fresh-context reviewer --> over
the spec's diff. It converges or reports why it stopped.

Write what it found to `plan.md ## Outstanding issues`, one row per finding in
the form `specs/_template/plan.md` gives. Zero Critical and zero Blocker rows
without a terminal disposition passes the gate: record `approved`, and Major
and Minor stay as follow-up signal. Otherwise record `needs_revision`, and the
work goes back. The gate reads the rows, never their absence — a deleted row
reads exactly like a finding no review filed.

## acceptance — blocking, end of implement, after review

`review` judges the diff. This one judges the promise: walk
`spec.md ## Acceptance criteria` in order and answer each from something run
or read, never from the diff looking right.

Each criterion lands in one of three states, and the third is the point:

| State | Means |
|---|---|
| passed | checked, and it holds |
| failed | checked, and it does not |
| unmeasured | not checked — no instrument, no environment, or nobody ran it |

**Unmeasured is not passed.** A criterion nothing answered is an open promise,
and recording it as met is how a spec ships on an unobserved claim. It blocks
approval exactly as a failure does: `approved` requires every criterion
passed.

The way out of an unmeasured criterion is to measure it, or to say plainly
that it cannot be measured here: `deferred`, with a rationale naming the
criterion and what would answer it. `deferred` is a decision the
log keeps; silence is not. It also stops the work short of `completed` — the
spec waits for the gate to re-fire `approved`, or it is abandoned or
superseded with the record saying which criteria went unanswered.

Answer every criterion `spec.md` carries: one left out is one the gate never
looked at, which is the omission the third state exists to name. Name the
criteria by their number from `spec.md`, so the next session reads which ones
stand rather than re-deriving them:

```
- 2026-01-15 · acceptance · needs_revision · criterion 3 fails on empty input; 5 and 6 need the staging environment; the rest passed
```

A criterion that cannot be checked was not a criterion — the specify phase
says to rewrite it until it can be. Finding one only here is a finding about
the spec, and it belongs in `plan.md ## Outstanding issues` like any other.

## resume — inline, on a dirty worktree

Fires when a resume finds uncommitted changes: the previous session left work
in flight and what it intended is not on disk. Show what changed and ask
whether to keep going, land it first, or discard. All three continue the
resume, so all three record `approved` with which was chosen; `deferred` when
the answer is to stop and look first.

The procedure resumes at the step that fired this — like every gate here, this
section ends at the decision and does not hand the order back.

A clean worktree resumes without this gate.
