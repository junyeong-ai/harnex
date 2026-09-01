---
paths:
  - ".claude/rules/**"
  - ".claude/skills/**"
governs:
  concept: when a harness artifact is added, promoted, or retired
  live_truth:
    - harness.toml
    - .claude/rules
    - .claude/skills
---

# Governance — when to add, promote, or retire a harness artifact

The harness improves by promoting recurring observations into rules, and by
retiring artifacts that stopped earning their context cost. The loop is
operator-driven and evidence-gated — never auto-applied, never AI-invented.

## Where observations accumulate

A candidate starts as an observation, not a rule. Record it where it survives
the session without spending always-loaded context:

- **Oracle ledger** —
  `harnex lifecycle observe --tag <topic> --text "<what recurs>" --source <where>`
  appends to the per-tag ledger the surfacing step below reads. This is the
  only home where recurrence is counted for you.
- **Commit body** — durable, and nothing surfaces it. `git log` keeps the
  trail, and a candidate recorded only there is found by a person who goes
  looking, never by `harnex lifecycle candidates`.

Recurrence is counted per `(tag, text)`, the text after case and whitespace
and the tag exactly as spelled. A constraint the ledger already knows,
rephrased or filed under a differently-spelled tag, starts a second count, and
neither entry then proves the recurrence both were recording — read the tag's
ledger before appending and reuse the standing wording. An empty tag is
refused: a record is surfaced under its tag, and one with no tag is one no
pass would ever reach.

Do not record observations in always-loaded memory — that pays context cost
every session for a candidate that has not earned a rule yet.

## Promotion gate

Pick the bar by what the artifact ENFORCES, not by how it is written.

**Advisory rule** (a path-scoped `.claude/rules/*.md`) — all four must hold:

1. **Invariant?** Enforces a boundary the model cannot self-verify, where a
   violation is irreversible or invisible. If the linter/formatter catches it,
   it is redundant.
2. **Recurring?** The same issue surfaced in ≥2 independent contexts. A
   one-off belongs in the commit, not a rule.
3. **Verifiable?** A reviewer confirms compliance by reading the output. Vague
   guidance ("write clean code") fails.
4. **Low false-positive?** Catch rate exceeds false-positive cost. Legitimate
   code that regularly trips it erodes trust.

**Enforced guardrail** (a hook or a `permissions.deny` rule) — non-bypassable,
so it clears a HIGHER bar: the four above PLUS

5. **Spec-cited** — names the Claude Code behavior it relies on (re-verified
   against the live docs, not memory).
6. **Mechanized + tested** — the rule lives in the oracle/template SSoT with a
   test, not as hand-authored control flow.
7. **Human-approved** — a person signs off; an enforced guardrail that
   misfires blocks real work, so it is never promoted by the model alone.

## Surfacing candidates (the loop)

`/harness-curate` runs this whole section: it drives the commands below, brings
each candidate to the promotion gate above, lands the decision, and records it.
Reach for it rather than working the steps by hand.

The commands are deterministic and never invent text:

- `harnex lifecycle candidates` — observations that crossed the configured
  instance + age thresholds, with the ledger they were drawn from. No
  candidates over an unwritten ledger is a loop whose first half never fired,
  not a corpus with nothing in it.
- `harnex telemetry report` — per-Kind counts, for reading the ledger itself.
  Never a retirement verdict: it counts Kinds, not artifacts.
- `harnex lifecycle retire` — Stale / NoConsumers / Silent verdicts. Silence
  is reported only for a kind that declares the record naming its artifacts;
  every other kind reads `unmeasured`, which is not a candidate.

Record each decision in a commit body, or with the oracle:
`harnex lifecycle {promote|reject|defer|demote} --tag <t> --text <text>
--decision-text "<rationale>"`. The decision text is the operator's, never
the model's.

## Rejection reasons

- Restates what the formatter or linter already enforces.
- Encodes a habit a capable model follows by default.
- Uses a natural-language pattern match in a blocking tier.
- Applies to a single package — use a path-scoped rule, not a project-wide one.

## Subagent lifetime follows role

A fan-out that leaves every finished context addressable accretes, and a
re-summoned context is not the fresh one its role promised.

- **Investigator / auditor** — one charge, one report, retired. Its context is
  not fresh for judging its own findings, so the next round spawns anew.
- **Fixer** — one per repository or independent track, continued by message
  across rounds. Re-spawning discards the file context that is its value.
- **Reviewer** — re-spawned fresh per round as `<scope>-r<N>`, never handed
  its own prior findings.

For the retiring roles the report is the agent's final message: the caller
consumes it and retires the agent. "Idle and addressable" is a re-summon
surface, not a resting state. A fixer's round report is a checkpoint.

Per round: one investigator or fixer per independent track, one reviewer per
repository. A session that exceeds that says why. All of it is Article VII
default, not floor.

## Break glass

Every enforced guardrail names its escape hatch, and using one is loud and
recorded. `.claude/settings.local.json` (gitignored, per-developer) is the
override home — `disableAllHooks: true` disarms the hook layer session-wide.
The committed `.claude/settings.json` carries only team-shared defaults, so a
fresh clone is policy-consistent with no per-developer setup.

A guardrail with no hatch gets bypassed by a worse route. A hatch with no
record is indistinguishable from a defect.

## Retirement

See `artifact-lifecycle.md` for retirement criteria, procedure, and the
foundation artifacts that are exempt.
