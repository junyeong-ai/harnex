---
description: "One forked, read-only lens walk over a change set — findings are the only output, and no file is modified. The fresh context is the point: the reviewer saw nothing of how the change came to be. Fix intent routes to the review skill instead."
when_to_use: "Invoke on findings-only asks — \"audit the diff\", \"critique this change\", \"what would a reviewer say\" — over a path, a revision range (main..HEAD), or a glob; also the confirmation pass a pipeline runs over a landed diff when the bookend trigger fires. Not for fix intent: \"review and fix\" is the review skill."
argument-hint: "<path> | <commit-range> | <glob>"
context: fork
agent: reviewer
background: false
---

# Critique

You run as the reviewer agent, and `.claude/agents/reviewer.md` is your
contract — regime selection, coverage, refutation, the close, delivery. This
charge adds only what varies per invocation: the scope.

Resolve it exactly as the review loop does: a path or glob names its files; a
revision range is `git diff --name-only <range>`; nothing is
`git diff --name-only HEAD`, the uncommitted working set. Pull each file's
prose sibling in — the rule whose `paths:` matches it and the nearest
`CLAUDE.md` above it. An empty scope is a complete result: say so and stop
rather than inventing one.

This is a change-set audit, so the code regime applies
([file: .claude/rules/review-lenses.md § Two refutation regimes, chosen by subject]).
Walk each lens in
`.claude/lenses/` over the files its `applies_to:` covers, and return findings
per the agent contract — nothing here loops, fixes, or writes; convergence
belongs to the caller that reads this report.
