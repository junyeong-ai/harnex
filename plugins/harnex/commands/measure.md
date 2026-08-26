---
description: Read your own Claude Code transcripts and report how you instruct it — what you repeated, where you stepped in, what each instruction left unspecified, and what to change
argument-hint: "[--since <rfc3339>] [--sample <n>]"
disable-model-invocation: true
allowed-tools: ["Bash(harness:*)", "Read", "Grep", "Task", "AskUserQuestion"]
---

Report how the operator instructs Claude Code, from the transcripts Claude Code
already wrote. The oracle decides what is countable; `session-judge` reads what
is not; this command joins them and never blurs which is which.

Requires `[session]` in `harness.toml`. Without the binary there are no
numbers — say so and stop rather than estimating from the logs by hand.

## 1 — Fix the window

`harness session baseline diff` names the last measured window. Start where it
ended, so this measurement and the last one do not overlap; with no baseline
yet, take the corpus whole. `--since` overrides.

## 2 — Take the facts

```
harness session facts --since <t>
harness session submissions --since <t> --with-text --sample <n>
```

`--sample` defaults to `[session] submission_sample`. Evidence the envelopes do
not carry does not exist — do not supply it from reading transcripts by hand.

## 3 — Judge the instructions

Dispatch `session-judge` over the sampled instructions, at most 25 per agent,
batches in parallel. It returns a gap and a rewrite per instruction, or null.
Its contract is in its own file; do not restate or relax it here.

## 4 — Find what recurs

**A gap that recurs is not a prompting habit, it is a missing harness.** If the
same constraint has to be supplied by hand across sessions, its home is
`CLAUDE.md` or a path-scoped rule, and `/harnex extend` is how it gets there.

Two inputs converge on the same conclusion and should be read together:
`prompts.repeated_blocks` (paragraphs retyped across sessions — never installed)
and the judge's recurring gaps (constraints never written down at all). The
first is what the operator already knows they repeat; the second is what they
do not.

## 5 — Report

Keep this order. A section with nothing in it says so and says why — an empty
section with its reason is worth more than a filled one that guessed.

1. **What this window was** — files, records, span, runtime versions, coverage.
2. **Delta** — `baseline diff` against the previous window. Absent on a first
   run; say "first measurement" rather than showing zeros.
3. **What was instructed** — instructions, length and agent-turn distribution,
   how many were cut short.
4. **Where the operator stepped in** — `interventions`, by kind.
5. **What was repeated** — across sessions (never installed) and within one
   session (did not survive its context). These are different failures.
6. **Where work was redone** — `post_commit_reedits`, per commit.
7. **What the harness cost** — hook wall-clock, rule-load characters, denials.
8. **What to change** — each item tagged `apply` (harnex can write it) or
   `report` (outside `${CLAUDE_PROJECT_DIR}`, so the operator writes it), and
   each carrying the metric that will show whether it worked.
9. **Limits** — state these with their numbers, every run:
   - marked interrupts are a floor, not a count
   - `user-rejected` denials are not refusals; the four causes behind that one
     wire value are separable only in message text
   - a denial cannot be attributed to the permission rule that caused it
   - a hook's cost is exact and its value is not recorded at all
   - §3's judged findings are readings by the model in `session-judge`, over
     the sample size, and they never enter a baseline

Write the report in the language the operator instructs in.

## 6 — Close the loop

Offer, do not run: `harness session baseline save --label <name>`. The next
measurement starts where this one ended, and §2 of the next report is the
answer to whether any of this worked.
