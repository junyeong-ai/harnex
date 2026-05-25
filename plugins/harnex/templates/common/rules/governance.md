---
paths:
  - ".claude/rules/**"
  - ".claude/skills/**"
---

# Governance — when to add, promote, or reject a rule

Before proposing a new rule, apply this rubric. A "no" on any question
means the proposal should be rejected or deferred.

## Promotion gate (observation → rule)

1. **Invariant?** Does the candidate enforce a boundary the model cannot
   self-verify — at a point where a violation is irreversible or invisible?
   If the linter or formatter already catches it, the rule is redundant.
2. **Recurring?** Has the same issue surfaced in at least two independent
   contexts (sessions, PRs, developers)? A single-occurrence fix belongs
   in the commit, not in a rule.
3. **Verifiable?** Can a reviewer confirm compliance by reading the output?
   Vague guidance ("write clean code") fails this test.
4. **Low false-positive?** Does the rule's catch rate exceed its
   false-positive cost? If legitimate code regularly triggers it, the rule
   erodes trust and gets ignored.

## Rejection reasons

- Restates what the formatter or linter already enforces.
- Encodes a habit a capable model follows by default.
- Uses a natural-language pattern match in a blocking tier.
- Applies to a single package — use a path-scoped rule instead of a
  project-wide one.

## Retirement trigger

A rule that has not contributed a finding or shaped a decision in 90 days
is a retirement candidate. See `artifact-lifecycle.md`.
