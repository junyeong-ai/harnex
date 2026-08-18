---
description: "Convergent review loop. Walks every lens in .claude/lenses/ over a scope, fixes what a re-runnable authority can confirm, re-walks the scope the fixes grew, and stops when no Critical or Blocker remains. Closes with one fresh-context reviewer that did not watch the loop form its opinion."
when_to_use: "Invoke on fix intent over a change set — \"review and fix\", \"audit the diff\", \"walk the lenses over <path>\", or as the engine a spec review gate delegates to. Takes a path, a commit range, or a glob. Skip when the ask is read-only: report the findings and stop rather than entering the loop."
argument-hint: "<path> | <commit-range> | <glob>  [--max-iter N]"
disable-model-invocation: true
allowed-tools: Read Edit Grep Glob Agent Bash(git diff *) Bash(git log *) Bash(git rev-parse *) Bash(git status *)
---

# Convergent review

Walk the lenses, fix what is safe to fix, re-walk what the fixes touched, stop
on a clean pass. The loop is the engine; `.claude/rules/review-lenses.md` owns
the severity and citation vocabulary and is the reference for every judgment
below.

Read [convergence.md](convergence.md) when the loop needs a decision the
sections here do not settle: stall detection, scope growth, the iteration cap,
and how the terminal reviewer scales.

## Resolve the scope

Parse the argument into a concrete file list:

- a path or glob — the files it names
- a commit range (`main..HEAD`) — `git diff --name-only <range>`
- nothing — `git diff --name-only HEAD`, the uncommitted working set

An empty list converges in zero passes. Say so and stop; do not invent a scope.

**Pull each file's prose sibling in with it.** A rule whose `paths:` frontmatter
matches a file in scope enters the scope too, as does the nearest `CLAUDE.md`
above it. Reviewing code without the prose that describes it is how a renamed
symbol keeps a paragraph that names the old one — the single most common finding
this addition surfaces, and one no lens can reach if the file is not open.

<!-- harnex-fill: any other prose surface this project pairs with code — a
     package doc, an OpenAPI file, a schema kept beside its migration -->

## One iteration

1. **Walk every lens** in `.claude/lenses/` over every file in scope. A lens
   that fires produces findings in the rule's format; a lens that fires on
   nothing is a result, not a skipped step.
2. **State the coverage before the verdict.** Name what was actually read —
   the files opened, the symbol search that ran, what a search could not reach.
   A pass that reports zero findings with its coverage shown is a complete
   result; the same report without it is an assertion.
3. **Refute each candidate before filing it.** Try to break the finding against
   the tree. Ground truth that contradicts it drops it. An attempt that settles
   nothing — a race no read can trigger, a branch no fixture reaches —
   **down-calibrates** rather than drops: keep it, note what blocked the check,
   and cap it at Major. Critical and Blocker are the tier that edits files and
   stops a gate, and neither may rest on a claim this pass could not establish.
4. **Fix only what an authority can re-run.** The severity table in the rule
   decides: a Critical or Blocker citing a rule slug, a lint code, or a named
   test is fixable here, because something other than this loop's own opinion
   confirms the result. A finding citing judgment is reported, never edited —
   that citation is the author's own opt-out.
5. **Grow the scope by what the fixes touched**, never shrink it.

## Termination

Stop when a full pass produces no Critical and no Blocker. Major and Minor
remain as signal and do not block.

Also stop when the pass makes no progress, or at the iteration cap
(default 5 — a circuit breaker, not the control). On either, report the reason,
the findings that remain, and what scope was covered. Then return the
conversation: the operator decides. Never report convergence for a loop that
stopped early.

## The terminal pass

A loop that reviews its own fixes is grading its own work with the context that
produced them. Before reporting convergence, spawn **one** reviewer subagent
over the final scope and let it reach its own verdict. Its value is that it did
not watch the loop form its opinion — a fresh context, not a different model.

Give it the scope, the lenses, and nothing about what the loop already
concluded. If it returns findings, they enter the loop as any other iteration
would. If it returns nothing where it should have returned a verdict, judge the
round on what it delivered and re-fire — a subagent finishing is not a result.

## Report

Per iteration: the coverage, the findings by severity, what was fixed, and what
the scope grew to. At the end: converged or why not, and every finding still
open. Write nothing outside the files the fixes touch — persistence of residual
findings belongs to whatever invoked this, not to the loop.
