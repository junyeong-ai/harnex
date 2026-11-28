# Convergence mechanics

- [The next pass reads what the fixes touched](#the-next-pass-reads-what-the-fixes-touched)
- [Stalled](#stalled)
- [A finding in the last fix](#a-finding-in-the-last-fix)
- [The record outlives the context](#the-record-outlives-the-context)
- [A fix is pinned or it is surfaced](#a-fix-is-pinned-or-it-is-surfaced)
- [A class that comes back](#a-class-that-comes-back)
- [A dispute](#a-dispute)
- [One whole read at the end](#one-whole-read-at-the-end)
- [No count stops the loop](#no-count-stops-the-loop)
- [What the loop never does](#what-the-loop-never-does)
- [Scaling the terminal pass](#scaling-the-terminal-pass)

Read when the loop needs a decision the SKILL body does not settle.

## The next pass reads what the fixes touched

A pass after the first reads what the previous pass's fixes touched: the files
they edited, the callers of any declaration whose signature or behaviour they
changed, and those files' prose siblings. Re-walking the files the loop already
walked, unchanged, asks the context that already answered the same question
again, and what it finds there is its own reading a second time.

Narrowing loses no file, because the terminal pass reads the whole change —
every file the resolved scope named and every file a fix touched — so a file
leaves a self pass without leaving the review. The self passes end when a pass
fixes nothing, since its successor would read nothing. Fixes that keep widening
what the next pass reads are fixes that are not converging, and the exits below
catch them.

## Stalled

A finding whose fix the gate undid, or whose fix did not apply, stays standing
with the attempt recorded beneath it (§ A fix is pinned or it is surfaced), and
the loop never re-attempts it. When the self passes end, read what stands: a
fixable Critical or Blocker left standing by any pass ends the loop stalled,
before the terminal pass. The loop has nothing left to try on it, so report it
with its attempt. Judged from the last pass alone, an unrelated fix that pass
also made would carry the standing Blocker past the check into a converged
report.

A pass that changed files keeps the loop going even if it repeats a finding:
the fix moved something the finding did not cover, and the next pass reads what
moved.

## A finding in the last fix

Stalled catches the loop with nothing left to try. The slower failure keeps
editing: every pass changes files and finds a new Critical or Blocker inside
what the previous pass's fix wrote. That loop is refuting its own repairs, not
converging on the change, and its count can hold level the whole way — which
is why no count comparison decides it.

Where a finding lands does. When a pass's Critical or Blocker sits in lines
the previous pass's fix wrote, undo or simplify that fix rather than adding a
condition to it: the answer to a fix that introduced a defect is a smaller
fix, never a second one layered over the first. Undoing is an edit, so it
answers to the same citation boundary as any fix — a judgment-cited finding in
the last fix escalates with the fix named instead — and the undone fix is
recorded on its finding's row the way a gate-broken one is (§ A fix is pinned
or it is surfaced), so no later pass re-derives it. If no simpler form holds,
escalate with both findings side by side. A finding outside the last fix is
the loop reaching what was already there, and the pass proceeds.

## The record outlives the context

Convergence state that lives only in conversation is erased by exactly what a
long review meets — a compaction, a session end, a re-invocation. So the
record goes where the invocation can keep it: spec-bound, in the spec's
decision log and `plan.md ## Outstanding issues`; standalone, the record rides
the terminal report, and keeping it across invocations belongs to whatever
invoked the loop — the skill writes nothing outside the files the fixes touch
(SKILL § Report). Two things make up the record, both append-only:

- one line per pass — pass number, findings by severity, files touched;
- every finding's terminal disposition, end-anchored on its row —
  `[fixed: what pinned it]`, `[refuted: the ground truth]`, or
  `[accepted: who accepted it and why]`.

A cleared finding keeps its row and gains its disposition. Prose that narrates
findings away — "these became inexpressible after the redesign" — leaves a gate
unable to tell a fix from a shrug, which is the shape this record exists to
prevent. A re-invocation over the same scope reads the record first: an
already-adjudicated finding re-enters as its disposition, not as new work for
a fresh pass to re-litigate.

## A fix is pinned or it is surfaced

The next fresh reviewer's opinion is not a regression gate; a check is. An
auto-fix for a behavioural finding lands together with what pins it, or the
finding is surfaced instead of fixed. Reach for the pin in this order: an
assertion added to the test that already exercises the contract, a lint the
project already runs, and a new test only where nothing covers the contract
yet. Pin the contract the finding broke, not the edit that repaired it — a
test of the edit fails on the next refactor and leaves with it, taking the
protection along. Measured without this, one mirror file took six
consecutive fix-commits, one arm per round, because nothing pinned "the two
sides agree over all inputs" and every fresh pass found the next case. A
convention fix whose authority already re-runs is its own pin and needs no
new one.

A fix the project's own gate then rejects is undone, and the finding keeps
its row unchanged — an indented plain line beneath a bullet continues the
row's text and so rewrites the finding, so the attempt is
recorded as its own bullet on the line beneath the row,
`- attempted: <what was tried> — broke <gate>`, never by editing the row or
hanging prose under it. The loop never re-attempts a recorded fix: without
the record, the next pass
re-derives the same fix from the same finding, breaks the same gate, and the
loop runs in place. It surfaces the finding with the attempt beside it.

## A class that comes back

A Critical or Blocker outside the last fix, of a class an earlier pass's fix
claimed to close, says that fix enumerated cases where the class has a shared
seam. Fix it at the seam, pinned by one check over the whole class (§ A fix is
pinned or it is surfaced), and name the earlier fix it supersedes. Where the
members share no seam, escalate with the earlier findings of the class beside
it: another case-by-case fix is the same fix again.

## A dispute

A Critical or Blocker the terminal pass files enters the fix path unless ground
truth refutes it. One the loop refutes goes on the record with its refutation,
and once the batch's fixable Critical and Blocker findings are fixed and
verified the loop ends disputed,
handing the operator each refuted finding with its refutation beside it: the
loop and a fresh context disagree, and a second terminal pass would be another
draw on the same disagreement, not its settlement.

## One whole read at the end

The terminal pass is the one read of the whole change by a context that did not
watch it form, and the loop asks for it once. Its fixes are verified the way
every fix is — a self pass over what they touched — and the loop ends there.

A second whole read does not confirm the first. Over more than a thousand
recorded review passes, Critical and Blocker findings per pass held between
about 0.2 and 0.4 from the first pass through the ninth: each whole read is
another draw at much the same rate, and a loop that repeats it until one comes
back clean stops by chance, after paying for every draw before it. Where a
change's failure would be expensive, widen the one terminal round along the
axes in § Scaling the terminal pass rather than repeating it. Where the fixes
made after it are large enough that the operator wants another whole read,
that is the operator's decision, recorded as one.

## No count stops the loop

The loop runs while its passes fix what an authority confirms, and returns to
the operator on what its passes find:

| Exit | When |
|---|---|
| converged | no fixable Critical or Blocker stands from any pass, and the terminal pass filed none or every one it filed is fixed and the self passes over those fixes ended with one that fixed nothing |
| stalled | the self passes end with a fixable Critical or Blocker standing — its fix undone by the gate, or not applied (§ Stalled) |
| diverged | a finding in the last fix that no simpler form removes (§ A finding in the last fix) |
| returned | a class an earlier fix claimed to close comes back with no shared seam (§ A class that comes back) |
| disputed | the terminal pass filed a Critical or Blocker the loop refutes, once its fixable Critical and Blocker findings are fixed and verified (§ A dispute) |

At every exit but converged, report the reason, the findings that remain and
the scope covered; the operator decides between another pass, a narrower
scope, and stopping. The self passes carry no count: each reads only what the
last one changed, so they end when the fixes do, and a loop still fixing
confirmed defects is doing what it is for. The terminal read is one by what it
is (§ One whole read at the end), not a cap a count could raise.

## What the loop never does

- **Never fix on judgment.** The citation decides, per the severity table in
  the `review-lenses` skill. A finding this loop cannot have confirmed
  is reported to a person.
- **Never silently narrow.** If cost forces a bound — a sampled directory, a
  skipped generated tree — say which, in the report. A bound nobody states
  reads as coverage.
- **Never grade its own convergence.** The terminal pass is a fresh context,
  and its absence is a loop that stopped, not a loop that converged — a
  recorded `Terminal verify: skipped — <reason>` on a trivial diff is a
  state, not an absence.

## Scaling the terminal pass

One fresh reviewer is the floor and the default: the load-bearing property is
context independence, not model tier, and a same-model second pass adds
redundancy, not signal. Where a change's failure would be expensive, scale
along the independence axis — each step opt-in, never the default path, and
an engine's unavailability never gates the default:

- **Partition by area** — one reviewer per subsystem when the scope spans
  several. Each partition still receives the whole diff and is charged with
  one area of it, because the defect a partitioned pass most likely misses is
  the one between two partitions — a caller not updated with its callee, a
  contract broken across a package edge.
- **Vary the lens, not the count** — where a finding could be wrong in more
  than one way, give each reviewer a different question rather than the same
  one twice. Redundancy catches noise; diversity catches classes.
- **Cross a provider boundary** — a different provider's engine over the same
  diff catches the blind spots a model family structurally shares with
  itself; a same-provider second model does not. A Critical or Blocker either
  engine raises and the other cannot refute enters the fix path.

Every engine gets the same charge: the scope,
`.claude/skills/review-lenses/SKILL.md` with its witness rule, and the close
`.claude/agents/reviewer.md` mandates. An
engine outside the reviewer agent reads those files rather than a paraphrase,
and none is told what an earlier round filed — an engine charged with
refuting its own previous findings is running that round again.

Under more than one reviewer the round's count is the union after
cross-refutation — one number per round, never one per engine.

Each reviewer is spawned once, reports, and is done. A reviewer asked to judge a
second round is no longer the fresh context that was its whole value.

Every axis above multiplies what a round spends. The bound on agents is the
runtime's, written in the
settings `env` block as `CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS` and
`CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH` (spec-facts, settings). Scaling along
more than one axis without them states a fan-out nothing bounds.
