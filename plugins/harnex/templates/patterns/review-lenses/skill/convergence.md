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
