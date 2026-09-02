---
paths:
  - ".claude/rules/**"
  - ".claude/skills/**"
  - ".claude/lenses/**"
governs:
  concept: the lifecycle every harness artifact moves through
  live_truth:
    - .claude/rules
    - .claude/skills
---

# Artifact lifecycle — promotion, retirement, hygiene

Every harness artifact (rule, skill, lens, hook verifier) has a lifecycle.
Artifacts that outlive their relevance waste context tokens on every session
and erode the signal-to-noise ratio of the harness.

## Promotion path

```
observation → validated pattern → rule / skill / lens
```

- **Observation**: a repeated issue noticed during work. Record in a commit
  body, or `harnex lifecycle observe --tag <topic> --text "<obs>" --source
  <where>` when the oracle is adopted. Never in always-loaded memory. No
  promotion yet.
- **Validated pattern**: the same observation confirmed across two+
  independent contexts. `harnex lifecycle candidates` surfaces the wordings
  that recurred on their own; `harnex lifecycle observations` lays every
  wording out by tag for the reader to cluster what the count could not.
  Propose as a rule via the governance rubric.
- **Rule / skill / lens**: accepted by governance; committed; enforced
  or advisory per the enforced-vs-advisory principle.

## Retirement criteria

An artifact is a retirement candidate when ALL of the following hold
(`harnex lifecycle retire` computes these deterministically when the oracle
is adopted; otherwise verify them by hand at a retro):
- No finding, decision, or reference attributed to it in 90+ days
  (Silent / Stale signals).
- Not listed as a foundation artifact (constitution, governance, this file).
- No active consumer (NoConsumers — grep the codebase for the slug; check
  backlinks).

## Retirement procedure

1. Move to a `demoted/` archive (or delete if no historical value).
2. Record the retirement in a commit body with rationale.
3. Remove references from CLAUDE.md, settings.json, or other artifacts.

## Exempt artifacts

Foundation artifacts are exempt from retirement — the always-loaded rules
(constitution, agent-conduct) and the ones that define this loop itself
(governance, artifact-lifecycle). Their removal requires an explicit
governance vote.
