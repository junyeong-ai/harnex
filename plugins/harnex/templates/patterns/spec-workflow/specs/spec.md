---
slug: <kebab-case-identifier>
title: <human-readable title>
status: active
created: <YYYY-MM-DD>
superseded_by:
---

# <title>

## Problem

<What is wrong or missing today, in terms an outsider can verify. Name the
observed symptom and where it was observed, not the solution.>

## Constraints

<What the solution may not do — existing contracts it must keep, budgets it
must fit, decisions already made elsewhere that bind this one.>

## Acceptance criteria

<Numbered, each independently checkable. The `acceptance` gate walks exactly
this list and answers each from something run or read — passed, failed, or
unmeasured — and an unmeasured one blocks approval as a failure does. So a
criterion nobody can check does not get waved through; it stalls the gate.
Write each one so an instrument can answer it.>

1. <observable outcome>
2. <observable outcome>

## Out of scope

<What a reader might reasonably expect here and will not find, so the review
gate does not read an omission as an oversight.>

## Decision log

<!-- One bullet per gate firing, appended, never rewritten. The orchestrator
     writes these; the format is
     `<YYYY-MM-DD> · <gate> · <token> · <counts>? · <rationale>`, the token is
     one of approved | rejected | needs_revision | deferred, and a counted
     firing carries its own class's counts — `<n>C/<n>B/<n>M/<n>m` for a
     review-class gate, `<n>P/<n>F/<n>U` for `acceptance`.
     `harnex plan audit` reads exactly this grammar. -->
