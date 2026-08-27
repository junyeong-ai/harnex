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
— say so and stop rather than estimating from the logs by hand, and name
`curl -fsSL https://github.com/junyeong-ai/harnex/raw/main/scripts/install.sh | bash`
as how it is installed. Enabling this plugin does not install it.

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

**If a cost source is installed, take it too.** The transcripts carry token
counts and no clock and no price: nothing in them says how long a tool call
took, whether it failed, or what any of it cost. A telemetry collector keyed by
`session_id` — `hatel` is the one this plugin knows — supplies exactly that
gap, and every citation here carries the session it belongs to, so the join is
exact. Ask it for the same window, check its own wiring first, and add:

| what it adds | why the transcript cannot |
|---|---|
| wall-clock per tool, and how often a tool *failed* | a tool result records what came back, not how long or whether it worked |
| money, split by model and by main / subagent | pricing is not in the record and is not this plugin's to know |
| active time per session | the transcript has timestamps, not attention |

This step is optional in both directions. With no collector the report is
complete without these rows and says so once; with one, no other section
changes its meaning. Never put money in a baseline — a price list moves on its
own, so a delta on cost is a delta on the price list.

## 3 — Judge the instructions

Dispatch `harnex:session-judge` over the sampled instructions, at most 25 per agent,
batches in parallel. Each entry returns a kind, and a gap with the clause that
closes it, or null. Its contract is in its own file; do not restate or relax it
here.

Report the share that came back `null`. A batch where nothing is null was not
read, and the same is true of a batch where everything is.

## 4 — Cross the judge's kinds with the outcomes the oracle observed

The kinds come from a model and the outcomes do not, which is what makes the
crossing worth reading: if the labels are wrong the observed strata still hold.
Report per kind — instructions, median `agent_turns`, `tokens.output`, share
cut short, share that shipped — and withhold a rate for any kind with fewer
instructions than `[session] min_support`.

Read `tools` beside `harness.denials`, which groups by the same tool names.
Friction is as much a function of which tool the work goes through as of how
broad a rule is: a window where most calls are the one tool permission rules
must guard will meet refusals whatever the rules say, and read-only tools meet
none. Say which it is before prescribing a rule change.

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

**Look at consecutive instructions, not only at each one.** Two shapes, and the
second is easy to miss. Where an instruction has `steered_away`, the operator
stopped the agent mid-run and said the next thing. Where one has `agent_turns:
0` and is not the last in its session, they replaced it before the agent moved
at all — no interruption is marked, because there was no agent turn to
interrupt, and `agent_turns` is the only field that shows it.

Either way what the second instruction added is a constraint that could have
been given up front, stated by the person who found it missing — the most
directly installable thing this whole report produces. Show the pair as the
operator's two messages with the seconds between them, and hand the judge those
pairs adjacent and in order.

Three more inputs converge and should be read together: `across_sessions`
(paragraphs retyped in a session that did not yet hold them — never installed),
`within_sessions` (retyped inside a session that already held them — did not
survive its context), and the judge's recurring gaps (constraints never written
down at all). The first is what the operator knows they repeat; the last is
what they do not. Each of the two carries the same `chars` and `blocks`.

A paragraph is usually both, and the two are not one problem. The first counts
what was never installed anywhere, and installing it is the whole fix. The
second counts what was typed again inside one session, and **the oracle cannot
say why** — a paragraph retyped after a compaction, one retyped into a session
whose rules already carry it, and one retyped to weight it for a single task
are three findings with three fixes. Open the citations and let the judge
separate them; where `compactions` is non-empty, the timestamps settle the
first case outright.

Report the two separately and never sum them — over one real corpus the split
is 15% to 72%, and reading the total as the first prescribes installation for
something installation does not touch.

**`across_sessions` is null where the window holds instructions from fewer
than two sessions**, which any window scoped with `--session` does. Say that
the question was not asked; a null there is not an answer of none.

**Then check whether the constraint is already installed before prescribing
that it be.** `rule_loads` lists the project memory the runtime attached to a
turn, which is the path-scoped kind; a rule loaded on every turn is in the
prompt from the start and appears nowhere in the transcript. Read the
project's own rule files before naming a paragraph uninstalled — measured over
one window, the paragraph retyped into the most instructions was already a
rule, in force the whole time.

## 6 — Report

Three questions, in this order. A section with nothing in it says so and says
why — an empty section with its reason is worth more than a filled one that
guessed.

**0. This window.** Files, records, span, runtime versions, coverage, scope.
Then the delta from `baseline diff`, or "first measurement" — never zeros.
Where `change` is null on every metric the window is too thin to compare: say
so with `support_floor` and the denominators beside it, because the operator's
next act is to keep working rather than to change anything.

**Then what it cost and what it produced, on one line each.** Elapsed, `tokens`
by its four counts, instructions, agent turns; against commits, files touched,
and what `repository` says still stands. A report on how someone works with an
agent that never says what the whole thing came to has skipped the question
they asked. Cross-check the count against `repository.commits_in_span` and say
it plainly when the two disagree.

**1. What was delegated, and how it went.** §4's portfolio. Then the three
most expensive moments, each opened from its citation and shown with the turns
around it: the longest run, the costliest interruption, the instruction that
was cut short and restarted. **People learn from cases, not from rates.**

Each case is an account, not a verdict: what was asked, where the work landed
(`submissions[].files` and `tools`), what shipped, and the judge's `carried`
and `gap` beside it. `carried` is the half an operator can act on immediately —
it is the wording to keep.

**Quote the instruction; never print its citation as though that were the
case.** A uuid identifies a record and describes nothing — a reader shown one
learns that something happened somewhere. Give the operator's own words, the
clock time, and what changed under them. The citation belongs in a footnote for
whoever wants to reopen the file, not in the sentence doing the work.

**2. What leaks every session.** §5, plus `interventions` by kind and
`post_commit_reedits` per commit. Compaction belongs here when present: report
tokens in and out, and that the runtime's `cumulative_dropped_tokens` is a
running total per session, so it is read from the last event and never summed.

**3. What survived.** Present only under `--project`, and only where that
project is a git work tree. `repository.by_fate` counts what the branch still
reaches; `reverted_by` names commits undone with `git revert`. Join it to
`submissions[].commits` to say which instruction's work did not last.

State both limits or do not report the section: `unreachable` means the branch
does not reach it — a rebase, an amend, a reset, or a branch never merged, and
a repository that squash-merges puts every feature commit there — and a change
undone by hand carries no revert trailer and is invisible.

**4. Whether the harness earns its place.** Report `sessions` and instructions
per session first — the shape of the work is what the rest is per. `invocations`
is what was actually called; an element the operator built and never invoked is only visible under
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
- a commit is a floor: the runtime records some and not others, so
  `repository.commits_in_span` is what `commits` is a floor against and
  anything per-commit reads high
- a hook's cost is exact and its value is not recorded at all
- token counts are counts, never money; and a delta across a window whose
  `models` set moved is a delta about the model as much as the operator
- `tools` counts calls, not time or success; those come from a cost source or
  from nowhere
- `written` names what `Write` and `Edit` were pointed at, so work done through
  a shell leaves nothing there; `committed` is where the work landed, and needs
  `--project` over a git work tree
- a merge changed nothing on its own, so it contributes no paths to `committed`
- `rule_loads` is the project memory the runtime attached to a turn. A rule
  loaded on every turn is never attached and is absent here, so this is a floor
  on what was in force
- `unreachable` is not undone, and a hand-undone change is not `reverted_by`
- §3's judged findings are readings by the model in `session-judge`, over the
  sample size, and they never enter a baseline

Write the report in the language the operator instructs in.

## 7 — Close the loop

Offer, do not run: `harness session baseline save --label <name>`. The next
measurement starts where this one ended, and §0's delta is the answer to
whether any of this worked.
