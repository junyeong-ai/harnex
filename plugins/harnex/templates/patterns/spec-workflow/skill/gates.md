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
- 2026-01-15 · review · needs_revision · 0C/2B/3M/1m · two Blockers in the migration path; see plan.md ## Outstanding issues
```

Append-only. A gate that fires three times leaves three bullets in order — the
history of a decision is the interesting part, and overwriting keeps only the
last one. `git log specs/<slug>/` is the timeline this rides on.

A counted firing writes its counts into the line, because the next firing
reads them. Two gate classes count different things and each owes its own
token:

| Class | Gates | Token | What must reach zero |
|---|---|---|---|
| review | `design_review`, `review` | `<n>C/<n>B/<n>M/<n>m` | Critical + Blocker |
| acceptance | `acceptance` | `<n>P/<n>F/<n>U` | failed + unmeasured |

One rule bounds both: a cycle — the firings since the gate last recorded
`approved` or `rejected` — holds a budget of `needs_revision` firings, which
the shipped pre-commit arm passes. A deferral pauses the cycle; it does not
start a new one. Reaching the budget is a report, not a verdict on the round:
a review that needs that many rounds is naming a unit too large to finish as
one. Answer it by settling the scope — split what is under review, or close
the cycle — never by writing a reason to go on, because no line in the log
lifts the budget.

Round-to-round counts are not compared. A round's count samples what one
reviewer found, and a descent to zero is flat or rising in places; what tells
a converging loop from a diverging one is where its findings land, which the
`design_review` event below states. The budget is computed from the log's own
lines by `harnex plan audit`, never recalled — a convergence floor nothing
computes is prose, and the measured failure of that shape is a gate that
recorded eleven firings while its rule said stop at the second. A firing
carrying the other class's token is a finding: the wrong token parses and
reads as a total, so a review token on an acceptance line reports zero
blocking while unmeasured criteria stand.

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
steered by the last round's verdicts is that round again. On a Critical or
Blocker: record `needs_revision`, revise, re-fire. On a clean report:
transcribe what remains into `## Outstanding issues`, record `approved`,
proceed.

Revise by the smallest change that removes the finding. A Critical or Blocker
that sits in what the previous revision wrote says that revision failed:
rework or remove it rather than adding a condition to it. Every condition
added to answer a round is new surface for the next round to refute, and a
loop that answers its findings that way reaches the budget rather than a
design.

Judge the round on the verdict the reviewer delivered, never on the spawn
having finished. A reviewer that returned nothing has produced no report;
re-fire rather than approve.

## review — blocking, end of implement

Delegate to <!-- harnex-fill: what this project reviews a diff with — the
`.claude/skills/review/` loop when the review-lenses pattern is installed,
otherwise the project's own review command or a fresh-context reviewer --> over
the spec's diff. It converges or reports why it stopped.

Zero Critical and zero Blocker passes the gate. Write what remains to
`plan.md ## Outstanding issues` — Major and Minor are follow-up signal, not
blockers — and record `approved`. Otherwise record `needs_revision` with the
count, and the work goes back.

A finding written to `## Outstanding issues` is one
`- [Critical|Blocker|Major|Minor] <finding>` row, and a row is never deleted.
Cleared, it ends with its terminal disposition — `[fixed: what pinned it]`,
`[refuted: the ground truth]`, `[accepted: who accepted it and why]`. The gate
passes on zero Critical/Blocker rows *without* a terminal disposition — a
condition `harnex plan audit` computes, never on the rows' absence, which
narration can fake: the pre-commit arm at `hooks/pre-commit.d/check-plan.sh`
blocks the commit that leaves one standing, deletes a row, or rewords one.

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
and recording it as met is how a spec ships on an unobserved claim. It counts
against approval exactly as a failure does, which is why one token carries
both: `<n>P/<n>F/<n>U`, and `approved` requires the last two at zero —
`harnex plan audit` refuses an approval that carries either.

The way out of an unmeasured criterion is to measure it, or to say plainly
that it cannot be measured here: `deferred`, with the counts and a rationale
naming the criterion and what would answer it. `deferred` is a decision the
log keeps; silence is not. It also stops the work short of `completed` — the
spec waits for the gate to re-fire `approved`, or it is abandoned or
superseded with the record saying which criteria went unanswered.

The three counts add up to the list `spec.md` carries, and `harnex plan audit`
holds them to it: a criterion left out of the token is one the gate never
looked at, which is the omission the third state exists to name.

Name the criteria by their number from `spec.md`, so the next session reads
which ones stand rather than re-deriving them:

```
- 2026-01-15 · acceptance · needs_revision · 4P/1F/2U · criterion 3 fails on empty input; 5 and 6 need the staging environment
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
