---
description: Read your own Claude Code transcripts and report what you delegated and how it went, what you leak every session, and whether your harness is an asset — with one thing to change
argument-hint: "[--since <rfc3339>] [--project <dir>] [--sample <n>]"
disable-model-invocation: true
allowed-tools: ["Bash(harnex *)", "Read", "Grep", "Task", "AskUserQuestion"]
---

Report how the operator works with Claude Code, from the transcripts Claude
Code already wrote. The oracle decides what is countable; `session-judge` reads
what is not; this command joins them and never blurs which is which.

Requires `[session]` in `harness.toml`, which a scaffolded harness ships and a
hand-written one may not:

```toml
[session]
roots = ["~/.claude/projects"]   # or $CLAUDE_CONFIG_DIR/projects, spelled out
```

`roots` has no built-in default on purpose — it is a machine-global path, and
one compiled in would be its author's layout running on someone else's machine.
`~` is the only expansion the field performs, so a relocated config directory is
written out in full. Probe by asking the binary its version, which is inside
the grant this command carries; a refused probe did not run and is not an
absent binary. Without the binary there are no numbers
— say so and stop rather than estimating from the logs by hand, and name
`curl -fsSL https://github.com/junyeong-ai/harnex/raw/main/scripts/install.sh | bash`
as how it is installed. Enabling this plugin does not install it.

## 1 — Fix the window

`harnex session baseline trend` — scoped with the same `--project` as this
run, because trend matches scope exactly — names the windows already
measured; the last one's `observed_to` is where the previous window ended.
Start at the instant after it, never at it: a window keeps the record on its
boundary, so that timestamp taken verbatim counts one record twice. Trend
answers from the first baseline on, where a two-window `diff` cannot yet;
with no baseline recorded for this scope, take the corpus whole. `--since`
overrides.

`--project <dir>` scopes everything to sessions run in that directory or below
it, which is the only scope where the repository can be consulted about what
survived. Without it the window spans every project on the machine.

## 2 — Take the facts

```
harnex session facts [--since <t>] [--project <dir>] --with-text
harnex session submissions [--since <t>] [--project <dir>] --with-text [--sample <n>]
```

`submissions` returns `coverage` beside its `submissions`, the same coverage
`facts` carries, so either result read alone still says which window it is.

`--sample` defaults to `[session] submission_sample`. **Absent, that returns
the window whole**, and §3 dispatches an agent per 25 of them. Divide before
dispatching and say the number then: measured, 97 instructions cost four agents
and 648k tokens, and a first run with no baseline takes the corpus whole, which
is every project on the machine unless `--project` narrows it. Above a handful
of batches, tell the operator the count and let them cap it. Evidence the
envelopes do not carry does not exist — do not supply it from reading
transcripts by hand.

**If a cost source is installed, take it too.** The transcripts time a run and
never a call, and they carry no price at all. A telemetry collector keyed by
`session_id` supplies exactly that gap, and every citation here carries the
session it belongs to, so the join is exact. Any collector that keys on it
will do — hatel's report tool is one that does, and any equivalent serves; ask
it for the same window, check its own wiring first, and add:

| what it adds | why the transcript cannot |
|---|---|
| wall-clock per tool | the runtime times a whole run, not the calls inside it |
| money, split by model and by main / subagent | pricing is not in the record and is not this plugin's to know |
| active time per session | the transcript has timestamps, not attention |

A collector often carries more than price: domain kinds the project itself
emits — gates, deploys, rule blocks — keyed by the same session. Take the
window's kinds beside its cost and report them as the collector's rows, never
folded into the oracle's: the join is exact at the session, and at the session
only.

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
Report per kind — instructions, median `agent_turns`, `tokens.output`,
`elapsed_ms`, and how many were cut short and how many shipped **as counts
against that kind's own instruction count**, never as a share. `min_support` is
the floor for subtracting two rates in a comparison and this is one window
described, where the denominator is on the page: applied here it withholds
everything, because a kind holds a fraction of a window and the smallest would
need a thousand instructions to clear it — measured over 97, the largest kind
was 28.

A median answers what a kind costs *each time*, which is the question for
every kind but one. `continue` — "keep going", "next round" — carries no
content of its own, so what it costs is a total: the share of the window's
`agent_turns` and `tokens.output` spent under instructions that added nothing
to what was already standing. Report that share, and never prescribe from it
alone — a long continuation is an operator letting good work run as often as
it is one who stopped saying where to stop.

Read `tools` beside `harness.denials`, which groups by the same tool names.
Friction is as much a function of which tool the work goes through as of how
broad a rule is: a window where most calls are the one tool permission rules
must guard will meet refusals whatever the rules say, and read-only tools meet
none. Say which it is before prescribing a rule change.

`tools` carries `calls` and `failed` per tool. A failed call is one that ran
and came back an error, which is a different population from `harness.denials`
— those were refused and never ran — so report them apart: friction from the
harness and friction from the work want opposite fixes.

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

**Say what the instrument looked at before saying what it found.**
`block_chars` against `authored_chars` is the share of the operator's writing
that was long enough to be a paragraph at all — measured, 93% and 96% over two
projects. The rest is invisible to both fields, and so is anything restated in
different words, because matching is exact by design: a similarity threshold is
a language-dependent constant and this module refuses one. So a low repetition
figure means either that little was repeated or that little was examined, and
only these two numbers together say which.

The floor is a length, and a character is not a constant amount of meaning: the
same instruction is two to three times shorter in a language that writes a word
in one or two characters than in one that spells it out. Say how many
instructions fell under it and what share of `agent_turns` they carried —
measured over a Korean-language window, 19 of 75 instructions and 17% of the
turns. Where that share is large the floor is set for another language and
belongs in `harness.toml`.

**Neither field says a paragraph is a constraint.** It is an exact paragraph
typed twice, which a pasted error, a spec excerpt and a code block all are.
Open the citations and read the text before calling any of them a rule.

**Then check whether the constraint is already installed before prescribing
that it be.** `rule_loads` lists the project memory the runtime attached to a
turn, which is the path-scoped kind; a rule loaded on every turn is in the
prompt from the start and appears nowhere in the transcript. Read the
project's own rule files before naming a paragraph uninstalled — measured over
one window, the largest of these paragraphs were that project's always-loaded
rule file in translation, in force the whole time.

**Then write what survived into the ledger, so the next window can count it.**
A judged finding is a reading and never enters a baseline — but a constraint
recurs across windows only if something holds it between them, and the
lifecycle ledger is that thing. Requires `[lifecycle]` in `harness.toml`,
which a scaffolded harness ships, and the window scoped with `--project`: the
ledger lives in this project's tree and holds the constraint's text verbatim,
so a machine-wide window would write other projects' instructions into it.
Unscoped, record nothing and say so. One observation per distinct constraint
the window surfaced:

```
harnex lifecycle observe --tag measure-repeat --text "<the paragraph>" --source "<one citation>"
harnex lifecycle observe --tag measure-gap    --text "<the addition>"  --source "<one citation>"
```

`measure-repeat` is an `across_sessions` paragraph the reading above found to
be a constraint, in the oracle's own grouping of it; `measure-gap` is a judged
gap, in the words of its `addition`. Record nothing from `within_sessions`
alone — installation does not touch it — and nothing the rule files already
carry. **Before writing, `harnex lifecycle observations --tag <t>` says what
the tag already holds.** The promoter groups by exact text after case and
whitespace, so a constraint the ledger knows, rephrased, is a new constraint
to it, and the recurrence this entry exists to prove never accrues — reuse the
standing words verbatim. One observation per constraint per window: the
within-window count is already in the report, and counting windows is the
ledger's job.

Then `harnex lifecycle candidates`. A pair that crossed the configured
thresholds has waited through enough windows to stop being a sighting: report
it as ready for `/harnex extend`, and leave the decision where the decision
ledger keeps it — with the operator, through the curate pass their harness
carries.

## 6 — Report

Five sections, in this order. A section with nothing in it says so and says
why — an empty section with its reason is worth more than a filled one that
guessed.

**0. This window.** `files_in_window` and records, span, runtime versions,
coverage, scope. Coverage is the authorship ratio and what could not be read —
`record_types_unconsumed` names what this binary skipped and belongs in an
appendix, not a report about the operator. `files_discovered` is the corpus the run opened to find them,
which is what it cost and not what it measured — a scoped window is routinely
a handful of files out of thousands.
**`method_change`, then `harness_change`, then the delta.** `method_change`
says whether the two windows were measured the same way — the same oracle build
and the same paragraph floor. A metric can become wrong without its definition
moving, so a delta across `changed` is a reading about the ruler before it is
one about the work, and the two rates are still worth reporting where the
difference between them is not. `harness_change` then says whether the thing
being tested moved: `unchanged` means whatever moved, a harness
change is not why; `changed` means one moved and the operator can ask git
which; `unknown` means a window did not record what it ran under. A delta
reported without both is an association presented as an effect.

Then the delta from `baseline diff` — never zeros, and with fewer than two
saved windows of this scope there is no pair yet: say "first measurement" on
the first run and "no comparable pair yet" on the second. The pair is the
ledger's two most recent windows of this scope: the window being measured is
not saved yet, so its own delta is the next run's §0, as §7 says. Where the ledger holds windows of more than one scope, name the
pair — `--from` and `--to`, by the labels trend returned — because a bare
diff takes the ledger's last two windows whatever scope they belong to.
Where `change` is null on every metric the window is too thin to compare: say
so with `support_floor` and the denominators beside it, because the operator's
next act is to keep working rather than to change anything. Where the ledger
holds more than two windows of this scope, `harnex session baseline trend`
— with the same `--project` the window was scoped to, because trend matches
scope exactly and a bare invocation returns the machine-wide windows — lays
them side by side; report the trajectory of any metric an earlier
prescription named, because "did it work" is a direction before it is a delta.

**Then what it cost and what it produced, on one line each.** Elapsed, `tokens`
by its four counts, instructions, agent turns; against commits, files touched,
and what `repository` says still stands. A report on how someone works with an
agent that never says what the whole thing came to has skipped the question
they asked. Cross-check the count against `repository.authored_in_span` and say
it plainly when the two disagree.

**1. What was delegated, and how it went.** §4's portfolio. Then the three
most expensive moments, each opened from its citation and shown with the turns
around it: the longest run, the costliest interruption, the instruction that
was cut short and restarted. **People learn from cases, not from rates.**

Each case is an account, not a verdict: what was asked, where the work landed
(`written`, `committed` and `tools`), what shipped, and the judge's `carried`
and `gap` beside it. `carried` is the half an operator can act on immediately —
it is the wording to keep.

**Quote the instruction; never print its citation as though that were the
case.** A uuid identifies a record and describes nothing — a reader shown one
learns that something happened somewhere. Give the operator's own words, the
clock time, and what changed under them. The citation belongs in a footnote for
whoever wants to reopen the file, not in the sentence doing the work.

**2. What leaks every session.** §5, plus `interventions` by kind and
`post_commit_reedits`. Say over how many distinct `commit` values it is spread
before saying it per commit: re-edits concentrated in one interval are one
commit called done early, and the same count spread across many is a habit of
declaring early — measured, 244 of 246 fell in a single interval, where
"commit later" is the wrong prescription. Compaction belongs here when present: report
tokens in and out, and that the runtime's `cumulative_dropped_tokens` is a
running total per session, so it is read from the last event and never summed.

**Report what a compaction cost only as a pair.** `recovery.after_compaction`
counts the main-thread turns a summary had to carry alone and the corrections
the operator made inside them; `recovery.elsewhere` counts the same two things
across the rest of the main thread. Give both rates in the same sentence or
give neither — a recovery rate alone reads as the compaction's fault when it is
the rate of the work that follows one. Neither span counts a subagent's turns,
which ran on their own context, so the two do not sum to every turn in the
window and must not be held against `tokens` or `tools`.

`compactions[].instruction_chars` says whether the operator told that
compaction what to keep (absent = the runtime compacted on its own, `0` = they
compacted and said nothing); under `--with-text`, `instruction` is what they
said. Do not report a with-instruction against without-instruction comparison:
absent means the runtime hit its own limit, so the two groups differ by who
decided as well as by what was said, and the smaller one is 11 of 317 over the
local corpus — under `min_support` before the confound is even reached.

Open the largest recurring paragraph and quote it the way section 1 quotes
its cases — the count names a leak and the paragraph is what leaks. Then
close the section with what §5 wrote to the ledger and what `candidates`
returned: a constraint past its thresholds is this section's finding with a
next act attached.

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
`--project`, where the tree can be listed. Read an agent's spawn count beside
the skill that owns its procedure: an agent dispatched an order of magnitude
more often than the loop built to drive it says the procedure is being run
ad hoc, without the convergence accounting the skill carries — measured, one
reviewer agent at 668 spawns against 13 invocations of its loop. Report the
ratio, not a verdict; the operator knows which dispatches were deliberate. `blocked` is where the harness and
their habits disagree — report the concentration first (attempts against
distinct calls), because diffuse friction points at a broad rule and repeated
friction points at a habit, and the prescriptions are opposite. Then hook
wall-clock against the `elapsed_ms` of the instructions in the window, which is
what says whether it is worth anything: a hook holding two seconds is most of a
ten-second instruction and none of a ten-minute one. Then rule-load characters.

Relativise against the operator, never against other people: "this instruction
ran 27 times your median" names an outlier without a population.

**Then one thing to change.** Not a list. Pick the prescription with the
largest measured cost, tag it `apply` (harnex can write it) or `report`
(outside `${CLAUDE_PROJECT_DIR}`, so the operator writes it), and name the
metric that will show whether it worked. An `apply` names the exact
invocation — a `/harnex extend` verb where one composes it — rather than
describing the change. Everything else goes in an appendix.

**Limits**, with their numbers, every run:

- marked interrupts are a floor, not a count
- `user-rejected` denials are not refusals; the four causes behind that one
  wire value are separable only in message text
- a denial cannot be attributed to the permission rule that caused it
- `blocked` lists only calls refused more than once; a single refusal is not a
  pattern and most refusals never repeat
- a commit is a floor: the runtime records some and not others, so
  `repository.authored_in_span` is what `commits` is a floor against — measured,
  41 of 115 over one project. A per-commit **rate** is not high for that reason:
  a re-edit is only found against a commit the window observed, so both sides
  of the ratio are over the same commits. Re-denominating it in
  `authored_in_span` assumes the unobserved commits were never re-edited
- a hook's cost is exact and its value is not recorded at all
- token counts are counts, never money; and a delta across a window whose
  `models` set moved is a delta about the model as much as the operator
- `tools` counts calls and the calls that came back an error, and `failed` is a
  floor — a result whose call is in another transcript is not attributed.
  Per-call time comes from a cost source or from nowhere; `elapsed_ms` is the
  span from an instruction to the last record made under it, so it holds the
  agent's work and not the wait before the next instruction. The runtime's own
  `turn_duration` is not read: it measures stop to stop, which over the local
  corpus reaches 245 hours for one record and charges a session left open to
  whoever spoke last
- `written` names what `Write` and `Edit` were pointed at, so work done through
  a shell leaves nothing there; `committed` is where the work landed, and needs
  `--project` over a git work tree. Both are absolute, so what an instruction
  wrote and did not ship is `written` minus `committed` — and a write outside
  the project, to a scratch directory or another repository, stays in that
  difference and is not work the project lost
- a merge changed nothing on its own, so it contributes no paths to `committed`
- `written` minus `committed` is bounded by the commits the transcript
  recorded, not by the commits made — measured, 41 of 115 over one project — so
  a file committed in an unobserved commit sits in that difference. It is a
  ceiling on what did not ship, never a count of wasted work, and no baseline
  metric is denominated in it for that reason
- repetition is exact-paragraph only, so a constraint restated in other words
  is invisible and `chars: 0` is not evidence that nothing leaks;
  `block_chars` against `authored_chars` is what says how much was examined
- a `SESSION_COVERAGE_BELOW_FLOOR` warning means part of the operator's writing
  arrived under a prompt source this binary does not recognise. Those
  instructions are still counted and their work still attributed; only the two
  rates taken over the text are withheld, and the window answers about
  everything else
- `rule_loads` is the project memory the runtime attached to a turn. A rule
  loaded on every turn is never attached and is absent here, so this is a floor
  on what was in force
- `files_discovered` is the corpus the run opened; `files_in_window` is what it
  answered about
- `by_fate` counts the commits the transcript recorded, which is a floor
  against `authored_in_span` — measured, 41 of 115 over one project and 1,724 of
  4,502 over another, so anything denominated in observed commits reads high
- an event the runtime wrote into two transcripts is counted once — a record
  by its uuid, a message by the transcript that wrote it — and
  `records_duplicated` says how many were discarded. One shape escapes: where
  the transcript that reported a message first had its stream cut, its partial
  count is the one kept. Seen at 201 of 285,233 messages on one corpus and at
  none on a later, smaller one, so it is a shape to know about rather than a
  rate to expect
- a path containing a control character is reported as git spells it, escaped
- `unreachable` is not undone, and a hand-undone change is not `reverted_by`
- §3's judged findings are readings by the model in `session-judge`, over the
  sample size, and they never enter a baseline; what §5 writes to the
  lifecycle ledger is such a reading, held as an observation for the promoter
  to count, never as a fact
- a collector prices a session and a kind names an instruction, so money per
  kind would need an apportionment rule this command does not have; the cost
  join stops at the session it is exact at

Write the report in the language the operator instructs in. Where the session
can publish a page, the same report may be delivered as one — same sections,
same order, and the text above remains the deliverable of record.

## 7 — Close the loop

The ledger already holds what this window found recurring (§5); the baseline
is the loop's other half. Offer, do not run: `harnex session baseline save
--label <name>`. The next measurement starts where this one ended. What this
run prescribes is tested by the window that runs under it, and that window is
only saved at its own close — so the answer to whether any of this worked
arrives at the §0 two runs out, and the next run's §0 answers the previous
prescription.

**Save before applying anything.** A baseline records the harness the window
ran under, and it reads the tree at save time — so a save deferred until the
prescription is applied and committed stamps this window with a commit none
of its sessions ran under, and the next diff answers `harness_change` about
exactly the change it is testing from a mislabeled window. The order is:
save, then apply, then commit — the window that tests the change is the next
one, and it starts after this save. A tree already carrying unrelated
uncommitted harness changes still records `uncommitted`, and a comparison
against that window cannot answer `unchanged`.

The observations §5 wrote are project files whatever the prescription's tag —
commit them this cycle, or the recurrence they exist to prove stays on one
machine.
