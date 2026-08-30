# Convergence mechanics

Read when the loop needs a decision the SKILL body does not settle.

## Scope grows, never shrinks

Each iteration re-walks the previous scope plus every file a fix touched, plus
those files' prose siblings. Monotonic growth is what makes termination mean
something: a loop that dropped files could converge by forgetting them.

The set is bounded in practice because a fix that keeps widening the scope is a
fix that is not converging, and the stall rule below catches it.

## Stalled

A pass is stalled when it produces the same findings as the previous pass and
changed no file. Stop there rather than at the cap — a loop with nothing left to
try is done, and spending the remaining iterations on it only delays the report.

Two passes producing the same finding while files DID change is not stalled: the
fix moved something the finding did not cover. Let it run.

## A count that will not fall

Stalled catches the loop with nothing left to try. The slower failure keeps
editing: files change every pass while the Critical + Blocker count stays level
or rises — each fix opening what the last one closed. A pass whose count did
not fall below the previous pass's escalates rather than riding to the cap;
riding on anyway takes the operator's own recorded acknowledgement naming why
another pass is justified — the same rule, at the same threshold, the
spec-workflow gates hold their re-fires to. The comparison reads the recorded
pass lines (below), never memory — a convergence floor nothing computes is
prose, and the measured failure of that shape is a gate that recorded eleven
firings while its own rule said stop at the second.

## The record outlives the context

Convergence state that lives only in conversation is erased by exactly what a
long review meets — a compaction, a session end, a re-invocation. So the
record goes where the invocation can keep it: spec-bound, in the spec's
decision log and `plan.md ## Outstanding issues`; standalone, the record rides
the terminal report, and keeping it across invocations belongs to whatever
invoked the loop — the skill writes nothing outside the files the fixes touch
(SKILL § Report). Two things make up the record, both append-only:

- one line per pass — pass number, findings by severity, files touched;
- every finding's terminal disposition — `fixed` (with what pinned it),
  `refuted` (with the ground truth), or `accepted` (with who accepted it).

A cleared finding keeps its row and gains its disposition. Prose that narrates
findings away — "these became inexpressible after the redesign" — leaves a gate
unable to tell a fix from a shrug, which is the shape this record exists to
prevent. A re-invocation over the same scope reads the record first: an
already-adjudicated finding re-enters as its disposition, not as new work for
a fresh pass to re-litigate.

## A fix is pinned or it is surfaced

The next fresh reviewer's opinion is not a regression gate; a check is. An
auto-fix for a behavioural finding lands together with what pins it — a test,
an extended assertion, a lint the project already runs — or the finding is
surfaced instead of fixed. Measured without this, one mirror file took six
consecutive fix-commits, one arm per round, because nothing pinned "the two
sides agree over all inputs" and every fresh pass found the next case.

A fix the project's own gate then rejects is reverted — and recorded as
attempted, with what failed. The loop never re-attempts a recorded failed fix;
it surfaces the finding with the attempt beside it.

## The cap

Default 5 iterations. It is a circuit breaker for the case the stall rule
cannot see — a fix that reopens a finding it just closed, a pair of findings
whose fixes undo each other. Reaching it is a report, never a pass.

Raise it with `--max-iter` when a scope is genuinely large. Do not raise it to
make a loop converge: a loop that needs 20 passes is telling you the change is
too big to review as one unit.

## What the loop never does

- **Never fix on judgment.** The citation decides, per the severity table in
  `.claude/rules/review-lenses.md`. A finding this loop cannot have confirmed
  is reported to a person.
- **Never silently narrow.** If cost forces a bound — a sampled directory, a
  skipped generated tree — say which, in the report. A bound nobody states
  reads as coverage.
- **Never grade its own convergence.** The terminal pass is a fresh context,
  and its absence is a loop that stopped, not a loop that converged.

## Scaling the terminal pass

One fresh reviewer is the floor and the default. Where a change deserves more,
the axis is independence, not volume:

- **Partition by area** — one reviewer per subsystem when the scope spans
  several and no single context holds it well.
- **Vary the lens, not the count** — where a finding could be wrong in more than
  one way, give each reviewer a different question rather than the same one
  twice. Redundancy catches noise; diversity catches classes.

Each reviewer is spawned once, reports, and is done. A reviewer asked to judge a
second round is no longer the fresh context that was its whole value.
