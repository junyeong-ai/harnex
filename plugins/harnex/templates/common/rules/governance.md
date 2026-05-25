---
paths:
  - ".claude/rules/**"
  - ".claude/skills/**"
---

# Governance — when to add, promote, or retire a harness artifact

The harness improves by promoting recurring observations into rules, and by
retiring artifacts that stopped earning their context cost. The loop is
operator-driven and evidence-gated — never auto-applied, never AI-invented.

## Where observations accumulate

A candidate starts as an observation, not a rule. Record it where it survives
the session without spending always-loaded context:

- **Commit body** — always available; `git log` is the durable trail.
- **Oracle ledger** (if the project runs the `harness` oracle) —
  `harness lifecycle observe --tag <topic> --text "<observation>" --source <where>`
  appends to the per-tag ledger that surfacing reads. Preferred when adopted:
  thresholds are enforced deterministically.

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

With the oracle adopted, run these periodically (e.g. at a retro) — they are
deterministic, never inventing text:

- `harness lifecycle candidates` — observations that crossed the configured
  instance + age thresholds.
- `harness telemetry report` — hit counts; a rule with zero activity is a
  retirement candidate.
- `harness lifecycle retire` — Stale / NoConsumers / Silent verdicts.

Record each decision in a commit body, or with the oracle:
`harness lifecycle {promote|reject|defer|demote} --tag <t> --text <text>
--decision-text "<rationale>"`. The decision text is the operator's, never
the model's.

## Rejection reasons

- Restates what the formatter or linter already enforces.
- Encodes a habit a capable model follows by default.
- Uses a natural-language pattern match in a blocking tier.
- Applies to a single package — use a path-scoped rule, not a project-wide one.

## Retirement

See `artifact-lifecycle.md` for retirement criteria, procedure, and the
foundation artifacts that are exempt.
