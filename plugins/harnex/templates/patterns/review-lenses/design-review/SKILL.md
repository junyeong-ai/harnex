---
description: "Forked refutation of a design document before code exists — read-only, default-refute, self-gated on the bookend trigger. A wrong decision found after it is built is the costliest correction, so this pass pays for a forked context exactly where the blast radius warrants one."
when_to_use: "Invoke before building from a design or plan document — \"design review plan.md\", \"refute this design\" — or as the gate step a spec pipeline runs before build. Takes the document's path. Consults the trigger first; no signal firing means the pass reports not required and stops."
argument-hint: "<design-doc path>"
context: fork
agent: reviewer
background: false
---

# Design review

You run as the reviewer agent, and `.claude/agents/reviewer.md` is your
contract — regime selection, coverage, refutation, the close, delivery. This
charge adds the gate, the subject, and the stall guard.

**Gate first.** Read the document and evaluate the bookend trigger
([file: .claude/rules/review-lenses.md § The bookend trigger]) against the surface
it declares. No signal firing: report `Design review: not required — no
trigger signal`, still closed per the agent contract — coverage names the
document and the consult, the VERDICT counts zero — and stop; the skipped
pass is the trigger earning its cost argument. A document that cannot be
read, or that declares no surface, fails closed: the pass fires — the
unreadable document is a Blocker, and a missing surface declaration is
itself a finding.

**Default-refute each design decision** — the design regime, because code
does not exist yet and the false pass is the expensive direction. Ground
every refutation in the tree: read the artifact the decision cites at its
`file:line`, or run a cheap synthetic check; never re-derive the design's own
logic and call the agreement verification. A decision you cannot ground-truth
as sound is a finding. Anchor each to an authority, or mark it judgment and
leave it to the operator. Use the lenses as questions to sharpen refutations
rather than walking them file by file — the lens walk is the change-set
instrument, and this subject is one document.

**A re-run guards against churn by set, not by count.** Form your own
findings first; only then read what the document already records — the one
sanctioned read of prior findings, and it shapes the stall verdict, never
your set. A round with no Critical and no Blocker of its own proceeds; it is
never a stall, whatever the record holds. Otherwise, a round whose Critical
and Blocker set is a subset of what is already recorded is a stall: report
the stall rather than the subset, because re-surfacing the recorded is churn
while a genuinely new class is the pass earning its cost — and the round
that finds the deepest defect is often a late one, which is why the guard is
a set comparison and never a round count.

After the coverage, findings, and close the agent contract mandates, end
with one line a pipeline can read: `Design review: proceed` when no Critical
or Blocker stands, `Design review: block` otherwise.
