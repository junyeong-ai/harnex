---
paths:
  - "<!-- harnex-fill: the glob this rule governs, from the verb's argument -->"
---

# <!-- harnex-fill: what this rule governs, as a noun phrase -->

<!-- harnex-fill: one sentence — the invariant, and why the model cannot
     self-verify it. A rule that restates what the formatter or type checker
     already catches is redundant and costs context every time these paths are
     touched. -->

<!-- harnex-fill: one bullet per invariant the code under these paths ALREADY
     enforces, each naming the file that enforces it as `path/to/file.ext:line`.
     The pointer is the point: it is what a reader checks, what `harness check`
     resolves, and what makes this a rule rather than an opinion.

     Derive, never invent. An invariant with no enforcer in the tree is not a
     rule yet — it is an observation, and `harness lifecycle observe` is where
     it goes until it has recurred in two independent contexts (see
     `.claude/rules/governance.md`). Writing it here instead is how a harness
     accumulates confident prose nobody can check. -->

## Escape hatch

<!-- harnex-fill: how to proceed when this rule is wrong for a case — the flag,
     the marker, or the person to ask. A rule with no hatch gets bypassed by a
     worse route; delete this section only if the rule is advisory enough that
     ignoring it needs no ceremony. -->
