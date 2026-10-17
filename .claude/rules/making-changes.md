# Making a change

The constitution owns the project invariants. This file owns the change workflow.

## Order of precedence, when they conflict

1. **Correctness.** Solve for every valid input. Reproduce suspected defects
   against the actual implementation; a test or review claim may be wrong.
2. **Proven design.** Use repository conventions and existing mechanisms,
   then established ecosystem practice.
3. **Simplicity.** Add only the complexity the current requirement needs.

## The next reader is an agent

- Match existing names and boundaries; keep registrations and entry points
  explicit and discoverable by search.
- Keep an invariant beside its enforcer and a test beside its subject.
- Turn repeated, decidable conventions into types, schemas, lints or tests.
  Constitution IX owns facts with multiple representations. For an open-ended
  requirement, state the rule and test representative boundary cases.

## Scope

Change what was asked and what it directly requires. Keep unrelated cleanup
separate. Introduce no speculative abstraction, configuration or extension
point. Propose an adjacent improvement rather than implementing it unasked.

## Failure

Validate untrusted input at the boundary and trust validated internal calls.
Surface failure explicitly; never hide a wrong state behind a fallback,
default or swallowed exception. Never bypass a gate, skip validation or delete
coverage to make a change pass.

## Verification

Use the repository's tests, lints, types and build commands. Reproduce a defect
before fixing it and inspect the responsible source. Claim completion with
commands actually run and their results; state verification limits explicitly.

For a guard claiming something cannot happen, introduce that violation and
observe a test failure, then restore it. Verify the same test passes before
mutation; a compilation failure or rejected runner argument proves nothing
about the guard. Mutate each independently guarded member separately and run
a valid input through the same harness to check false positives. A behavior
assertion already proves its result and needs no redundant mutation exercise.

Run checks appropriate to the changed contracts. Broaden testing for a new
failure or unresolved risk, not to repeat evidence already established.

## Commits

Keep each decision revertible. Remove replaced code and update its consumers
in the same change, without compatibility shims, dead branches or historical
comments. Supersede decision records instead of deleting them.

Announce changes to an external contract: public API, schema, config, CLI or
what a gate reports on unmodified input. A newly detected pre-existing defect
is a correction. Other changes require a minor version bump in the same
release: `[meta] harnex_version` is the compatibility boundary enforced by
`Config::validate`. `.claude/skills/release/SKILL.md` owns the patch exception
for withdrawing a regression already shipped inside a pinned range.

Record a self-contained reversible change in one line. For a shared-state,
boundary or defect change, record the root cause, scope, invariant and actual
verification.
