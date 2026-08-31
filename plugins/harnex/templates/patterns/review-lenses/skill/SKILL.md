---
description: "Convergent review loop. Walks every lens in .claude/lenses/ over a scope, fixes what a cited authority confirms, re-walks the scope the fixes grew, and stops when no Critical or Blocker remains and one fresh-context reviewer that did not watch the loop form its opinion agrees. Modifies files — findings-only audits live in the critique skill."
when_to_use: "Invoke on fix-and-converge intent over a change set — \"review and fix\", \"converge the review\", \"리뷰 수렴해줘\", \"walk the lenses over <path>\" — or as the engine a spec review gate delegates to. Takes a path, a revision range (main..HEAD), or a glob. Read-only asks — findings without edits — route to the critique skill instead of entering the loop."
argument-hint: "<path> | <commit-range> | <glob>  [--max-iter N]"
allowed-tools: Read Edit Grep Glob Agent Bash(git diff *) Bash(git log *) Bash(git rev-parse *) Bash(git status *)
---

# Convergent review

Walk the lenses, fix what is safe to fix, re-walk what the fixes touched, stop
on a clean pass. The loop is the engine; `.claude/rules/review-lenses.md` owns
the severity and citation vocabulary, the authorities, and the refutation
regimes, and is the reference for every judgment below.

Read [convergence.md](convergence.md) when the loop needs a decision the
sections here do not settle: stall detection, scope growth, the iteration cap,
failed fixes, and how the terminal pass scales.

## Resolve the scope

Parse the argument into a concrete file list:

- a path or glob — the files it names
- a revision range (`main..HEAD`) — `git diff --name-only <range>`
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

1. **Walk each lens** in `.claude/lenses/` over the files in scope its
   `applies_to:` covers — a lens scoped to source has nothing to say about a
   spec, and walking it there is how a loop manufactures findings. A lens that
   fires produces findings in the rule's format; a lens that fires on nothing is
   a result, not a skipped step. Say which lenses were walked over what, since
   a narrowed lens is a narrowed pass.
2. **State the coverage before the verdict.** Name what was actually read —
   the files opened, the symbol search that ran, what a search could not reach.
   A pass that reports zero findings with its coverage shown is a complete
   result; the same report without it is an assertion.
3. **Refute each candidate before filing it**, under the code regime the rule
   defines: ground truth that contradicts a finding drops it; an attempt that
   settles nothing down-calibrates it to Major with a note naming what blocked
   the check.
4. **Fix only what an authority confirms.** The severity table in
   `.claude/rules/review-lenses.md` decides, and its authorities column is the
   definition — including the sources this project added at install.
   Re-listing them here would be the copy that refuses a finding citing an
   authority the project declared. A finding citing judgment is reported,
   never edited: that citation is the author's own opt-out.
5. **Run the project's fast gate over the pass's fixes.** A failure is triaged
   to the offending fix, which is undone by applying the inverse of the edit
   that produced it — the loop knows exactly what it changed, and no git
   restore can tell a fix from the operator's own uncommitted work in the
   same file, so nothing here touches git state. The finding keeps its row,
   with the attempt recorded beneath it (convergence.md § A fix is pinned or
   it is surfaced); the other fixes stand. An inverse that no longer applies
   — a later fix overlapped it — escalates that fix as judgment rather than
   improvising a wider undo. A transient flake retries once; a second flake
   escalates that fix as judgment.
6. **Grow the scope by what the fixes touched**, never shrink it.

<!-- harnex-fill: the fast gate command step 5 runs — name it here and grant
     it in allowed-tools above -->

## Termination

Stop when a full pass leaves no Critical and no Blocker the loop may fix. A
Critical or Blocker the loop may not fix — judgment-cited, or citing an
authority the rule's column does not know — is resolved by escalation rather
than edit: surface it with the convergence report for the operator's
disposition, because re-walking cannot close what the loop is forbidden to
fix, and counting it as failure makes convergence unreachable by
construction. Major and Minor remain as signal and do not block.

Also stop when the pass makes no progress, or at the iteration cap
(default 5 — a circuit breaker, not the control). On either, report the reason,
the findings that remain, and what scope was covered. Then return the
conversation: the operator decides. Never report convergence for a loop that
stopped early.

## The terminal pass

A loop that reviews its own fixes is grading its own work with the context
that produced them, and that failure mode is measured, not hypothetical: a
self-confirming loop declares done while a fresh context still finds a
load-bearing defect. Before reporting convergence, dispatch **one** reviewer
subagent (`.claude/agents/reviewer.md`) over the final scope and let it reach
its own verdict. Give it the scope, the lenses, and nothing about what the
loop concluded. Its value is a fresh context, not a different model.

Skip it only for a trivial, one-line, reversible diff — a fresh pass there
costs more than it catches. Every convergence report carries a
`Terminal verify:` line — the reviewer's verdict, or the skip with its
reason. The state is never implicit.

**A completion is not a result.** A reviewer that found nothing says so in
the close its contract mandates; a spawn that finished without a report did
not run the walk, and it satisfies "no Critical or Blocker" the same way a
clean pass does. Re-dispatch it once; if it again delivers no report, or
errors, or is unavailable, escalate with the degradation named
(`Terminal verify: unavailable — <reason>`) — never a silent convergence,
and never a quietly substituted reviewer.

**What comes back branches on its citation.** Critical or Blocker findings
citing an authority the rule's column knows enter the fix path as a new
iteration — not a blind re-walk, which already missed them once. At most two
such verify rounds; a blocker still standing after the second escalates. A
citation naming an authority the column does not know degrades to surfaced,
per the rule's conservative boundary. A judgment-cited Critical or Blocker
escalates its whole batch to the operator immediately — it cannot be
auto-fixed, and a re-walk would drop it; judgment findings at Major and
below ride the report as signal.

## Report

Per iteration: the coverage, the findings by severity, what was fixed, and what
the scope grew to. At the end: converged or why not, the `Terminal verify:`
line, and every finding still open. Write nothing outside the files the fixes
touch — persistence of residual findings belongs to whatever invoked this, not
to the loop.
