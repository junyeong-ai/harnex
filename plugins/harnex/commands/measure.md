---
description: Read your own Claude Code transcripts and report what you delegated and how it went, what you leak every session, and whether your harness is an asset — with one thing to change
argument-hint: "[--since <rfc3339>] [--project <dir>] [--sample <n>]"
disable-model-invocation: true
allowed-tools: ["Bash(harness:*)", "Read", "Grep", "Task", "AskUserQuestion"]
---

Report how the operator works with Claude Code, from the transcripts Claude
Code already wrote. The oracle decides what is countable; `session-judge` reads
what is not; this command joins them and never blurs which is which.

Requires `[session]` in `harness.toml`. Without the binary there are no numbers
— say so and stop rather than estimating from the logs by hand.

## 1 — Fix the window

`harness session baseline diff` names the last measured window. Start where it
ended, so this measurement and the last one do not overlap; with no baseline
yet, take the corpus whole. `--since` overrides.

`--project <dir>` scopes everything to sessions run in that directory or below
it, which is the only scope where the repository can be consulted about what
survived. Without it the window spans every project on the machine.

## 2 — Take the facts

```
harness session facts --since <t> [--project <dir>] --with-text
harness session submissions --since <t> [--project <dir>] --with-text --sample <n>
```

`--sample` defaults to `[session] submission_sample`. Evidence the envelopes do
not carry does not exist — do not supply it from reading transcripts by hand.

## 3 — Judge the instructions

Dispatch `session-judge` over the sampled instructions, at most 25 per agent,
batches in parallel. Each entry returns a kind, and a gap with a rewrite or
null. Its contract is in its own file; do not restate or relax it here.

## 4 — Cross the judge's kinds with the outcomes the oracle observed

The kinds come from a model and the outcomes do not, which is what makes the
crossing worth reading: if the labels are wrong the observed strata still hold.
Report per kind — instructions, median `agent_turns`, `tokens.output`, share
cut short, share that shipped — and withhold a rate for any kind with fewer
instructions than `[session] min_support`.

`tokens` carries four counts and no total, because they price differently by
orders of magnitude and this command does not know a price list. Rank on
`output`, name the others when they matter, and never convert to money.
Compare token counts across kinds only where `models` matches: a kind answered
by a different model is a different price, not a different habit.

This is the delegation question, and it is a portfolio rather than a score.
Say where the operator intervenes most and show it; do not say what they should
delegate, because a kind that draws steering may be collaborative by nature
rather than badly delegated.

## 5 — Find what recurs

**A gap that recurs is not a prompting habit, it is a missing harness.** If the
same constraint has to be supplied by hand across sessions, its home is
`CLAUDE.md` or a path-scoped rule, and `/harnex extend` is how it gets there.

Three inputs converge and should be read together: `prompts.repeated_blocks`
(paragraphs retyped across sessions — never installed), `restated_blocks` (the
same paragraph twice inside one session — did not survive its context), and the
judge's recurring gaps (constraints never written down at all). The first is
what the operator knows they repeat; the last is what they do not.

Where `compactions` is non-empty, read it against `restated_blocks` by
timestamp: a paragraph retyped after a compaction is what that compaction cost.

## 6 — Report

Three questions, in this order. A section with nothing in it says so and says
why — an empty section with its reason is worth more than a filled one that
guessed.

**0. This window.** Files, records, span, runtime versions, coverage, scope.
Then the delta from `baseline diff`, or "first measurement" — never zeros.

**1. What was delegated, and how it went.** §4's portfolio. Then the three
most expensive moments, each opened from its citation and shown with the turns
around it: the longest run, the costliest interruption, the instruction that
was cut short and restarted. **People learn from cases, not from rates.**

**2. What leaks every session.** §5, plus `interventions` by kind and
`post_commit_reedits` per commit. Compaction belongs here when present: report
tokens in and out, and that the runtime's `cumulative_dropped_tokens` is a
running total per session, so it is read from the last event and never summed.

**3. Whether the harness earns its place.** `invocations` is what was actually
called; an element the operator built and never invoked is only visible under
`--project`, where the tree can be listed. `blocked` is where the harness and
their habits disagree — report the concentration first (attempts against
distinct calls), because diffuse friction points at a broad rule and repeated
friction points at a habit, and the prescriptions are opposite. Then hook
wall-clock and rule-load characters.

Relativise against the operator, never against other people: "this instruction
ran 27 times your median" names an outlier without a population.

**Then one thing to change.** Not a list. Pick the prescription with the
largest measured cost, tag it `apply` (harnex can write it) or `report`
(outside `${CLAUDE_PROJECT_DIR}`, so the operator writes it), and name the
metric that will show whether it worked. Everything else goes in an appendix.

**Limits**, with their numbers, every run:

- marked interrupts are a floor, not a count
- `user-rejected` denials are not refusals; the four causes behind that one
  wire value are separable only in message text
- a denial cannot be attributed to the permission rule that caused it
- `blocked` lists only calls refused more than once; a single refusal is not a
  pattern and most refusals never repeat
- a hook's cost is exact and its value is not recorded at all
- token counts are counts, never money; and a delta across a window whose
  `models` set moved is a delta about the model as much as the operator
- §3's judged findings are readings by the model in `session-judge`, over the
  sample size, and they never enter a baseline

Write the report in the language the operator instructs in.

## 7 — Close the loop

Offer, do not run: `harness session baseline save --label <name>`. The next
measurement starts where this one ended, and §0's delta is the answer to
whether any of this worked.
