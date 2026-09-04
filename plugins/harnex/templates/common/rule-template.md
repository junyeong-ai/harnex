---
paths:
  - "<!-- harnex-fill: the glob that loads this rule, from the verb's argument -->"
governs:
  concept: "<!-- harnex-fill: what this rule is truth about, as a noun phrase -->"
  live_truth:
    - "<!-- harnex-fill: the literal paths this rule describes — where its enforcers live. Loading (paths) and describing (live_truth) differ where the mechanism earns its place -->"
---

# <!-- harnex-fill: what this rule governs, as a noun phrase -->

<!-- harnex-fill: one sentence — the invariant, and why the model cannot
     self-verify it. A rule that restates what the formatter or type checker
     already catches is redundant and costs context every time these paths are
     touched. -->

<!-- harnex-fill: one bullet per invariant the code under these paths ALREADY
     enforces, each naming the file that enforces it as a marked claim.
     Name what the file spells: `[file: path/to/file.ext :: def load]` for a
     definition, `[file: .claude/rules/other.md § Escape hatch]` for a
     document's section, `[file: path/to/file.ext]` where the whole file is
     the owner. Each of those fails on the rename that invalidates it.
     `[file: path/to/file.ext:42]` is for a place inside a body that no name
     identifies — it only ever proved the file was that long.
     The pointer is the point: it is what a reader checks, what `harnex check`
     resolves, and what makes this a rule rather than an opinion.

     Derive, never invent. An invariant with no enforcer in the tree is not a
     rule yet — it is an observation, and `harnex lifecycle observe` is where
     it goes until it has recurred in two independent contexts (see
     `.claude/rules/governance.md`). Writing it here instead is how a harness
     accumulates confident prose nobody can check. -->

## Escape hatch

<!-- harnex-fill: how to proceed when this rule is wrong for a case — the flag,
     the marker, or the person to ask. A rule with no hatch gets bypassed by a
     worse route; delete this section only if the rule is advisory enough that
     ignoring it needs no ceremony. -->
