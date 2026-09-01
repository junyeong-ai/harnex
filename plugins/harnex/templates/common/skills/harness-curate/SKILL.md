---
description: "Periodic harness hygiene pass: surface what the oracle's ledgers have accumulated, judge each candidate against the governance rubric, land the promotions and retirements, and record every decision. Reads `harnex lifecycle candidates`, `harnex telemetry report` and `harnex lifecycle retire`; writes rules, archives artifacts, and appends decisions."
when_to_use: "Run on the team's own cadence — a retro, a release, the end of a spec — or when the corpus has visibly drifted: a rule cites a symbol that no longer exists, two rules state the same contract, the same retirement candidate keeps reappearing. Manual only, because it edits rules and archives artifacts and an unintended pass is not undone by re-running it. Distinct from `harnex check` (the gate, run every commit) and `harnex audit` (spec drift in generated wiring, read-only) — this is the corpus, and it writes."
argument-hint: "[--tag <topic>]  — omit to sweep every tag"
disable-model-invocation: true
allowed-tools: Read Edit Write Glob Grep AskUserQuestion Bash(harnex *) Bash(git log *) Bash(git show *) Bash(git diff *) Bash(git status *) Bash(git add *) Bash(git commit *) Bash(git mv *)
---

# harness-curate

The promotion-and-retirement loop `.claude/rules/governance.md` describes,
executed. That rule owns the rubric and the thresholds; this skill drives the
commands and lands the results, and never restates a bar the rule already sets.

## 1. Surface

Deterministic, and none of it invents text:

```
harnex lifecycle candidates    # observations past the instance + age thresholds
harnex telemetry report        # per-Kind counts; never a retirement verdict
harnex lifecycle retire        # Stale / NoConsumers / Silent verdicts
```

An empty result is a finished pass. Say so and stop — a sweep that always
finds something is a sweep that has started inventing.

## 2. Judge

Read `.claude/rules/governance.md` and apply its promotion gate to each
candidate, and `.claude/rules/artifact-lifecycle.md` to each retirement
verdict. Two failure modes to name out loud rather than resolve silently:

- **A candidate that only the model wants.** The rubric's bar is evidence
  across independent contexts. One vivid session is an observation.
- **A retirement verdict on a foundation artifact.** Those are exempt. If the
  sweep keeps proposing one, the exemption list is what needs the edit.

Bring the operator the candidates and your reading of each. The decision text
is theirs, not yours — record what they said, not a paraphrase that improves it.

## 3. Land

**Promoting**: write the artifact in the tier the rubric assigns —
enforced-vs-advisory decides between a hook, a `permissions.deny` rule, and a
path-scoped rule. A promotion that lands in the wrong tier is worse than none:
guidance in a blocking gate produces false positives, and an invariant in prose
produces nothing.

**Retiring**: move the artifact to the archive, then remove every reference to
it — `CLAUDE.md`, `.claude/settings.json`, other rules, hook wiring. Grep the
slug before you claim it is gone. An artifact that is archived but still cited
is worse than one that was left alone, because the citation now leads nowhere.

## 4. Record

```
harnex lifecycle {promote|reject|defer|demote} --tag <t> --text <text> \
  --decision-text "<the operator's rationale, verbatim>"
```

Then one commit per decision, so a promotion that turns out wrong is revertible
without unpicking the rest of the pass.

## 5. Verify

`harnex check` must pass on the result. A pass that leaves the harness failing
its own gate has not finished.
