---
paths:
  - ".claude/rules/**"
  - ".claude/skills/**"
  - ".claude/lenses/**"
---

# Artifact lifecycle — promotion, retirement, hygiene

Every harness artifact (rule, skill, lens, hook verifier) has a lifecycle.
Artifacts that outlive their relevance waste context tokens on every session
and erode the signal-to-noise ratio of the harness.

## Promotion path

```
observation → validated pattern → rule / skill / lens
```

- **Observation**: a repeated issue noticed during work. Record in
  auto-memory or a commit body. No promotion yet.
- **Validated pattern**: the same observation confirmed across two+
  independent contexts. Propose as a rule via the governance rubric.
- **Rule / skill / lens**: accepted by governance; committed; enforced
  or advisory per the enforced-vs-advisory principle.

## Retirement criteria

An artifact is a retirement candidate when ALL of the following hold:
- No finding, decision, or reference attributed to it in 90+ days.
- Not listed as a foundation artifact (constitution, governance, this file).
- No active consumer (grep the codebase for the slug; check backlinks).

## Retirement procedure

1. Move to a `demoted/` archive (or delete if no historical value).
2. Record the retirement in a commit body with rationale.
3. Remove references from CLAUDE.md, settings.json, or other artifacts.

## Exempt artifacts

Foundation artifacts are exempt from retirement: constitution, governance,
artifact-lifecycle. Their removal requires an explicit governance vote.
