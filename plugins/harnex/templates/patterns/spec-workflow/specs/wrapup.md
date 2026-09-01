# <title> — Wrapup

Written at the wrapup phase, once `review` and `acceptance` have both recorded
`approved`. Set the `spec.md` frontmatter `status` to `completed` in the same
commit.

`completed` means the spec's promise was kept and observed, so an `acceptance`
that ended `deferred` does not reach here: the spec stays in flight until the
gate re-fires `approved`, or it is abandoned or superseded with the record
saying which criteria went unanswered.

## Result

<What now holds that did not before. The `acceptance` gate already walked the
criteria and its verdict is in the decision log; this records the durable half
for a reader who will not open the log — what each criterion got.>

## What the work revealed

<Observations from doing it — where the plan was wrong, what took longer than
expected, what the codebase turned out to be. This is the raw material the
promotion gate reads; write what happened, not what should have.>

## Harness proposals

<Candidate rules, hooks, or skills this spec argues for. Each needs the
governance bar — recurring across two independent contexts, verifiable by
reading output, low false-positive — or it stays an observation.>

Record each one where it survives without spending always-loaded context (see
`.claude/rules/governance.md`); do not add it to a rule from here.
