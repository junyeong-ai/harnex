---
paths:
  - ".claude/lenses/**"
  - ".claude/skills/**"
---

# Review lens framework

A convergent review loop walks every registered lens over a change set,
ranks findings by severity, proposes fixes for the high-severity ones, and
re-walks the (possibly grown) scope until convergence or a stall limit.

Lens findings are advisory JUDGMENTS, not mechanically-verifiable checks — a
lens calls "premature abstraction" or "wrong name" by reasoning, not by a
deterministic rule. Per keep-soften-cut, a prose judgment must never drive a
silent auto-edit: severity here is PRIORITY, not auto-fixability. The loop
proposes; the operator (or the agent, with the change visible and approved)
applies. Reserve unattended auto-fix for the formatter / linter, never a lens.

## Loop semantics

1. Walk every lens in `.claude/lenses/` over the input scope.
2. Rank findings by severity; for Critical/Blocker, propose a concrete fix
   with its citing anchor for operator approval. Major/Minor → report.
3. Apply approved fixes, then re-walk the (possibly grown) scope.
4. Stop when no approved-and-unaddressed Critical/Blocker remain, OR the
   stall limit is reached (default 3 iterations).

## Default lenses

Six lenses ship as the baseline review vocabulary. Each leads with a
high-signal review question and may add a few clarifying facets — never a
linter-style exhaustive checklist or a list of model-default checks (those
belong to the formatter, type checker, and the model's own defaults, per
keep-soften-cut). Add, remove, or customize lenses to match your project's
priorities.

| Lens | High-signal question |
|---|---|
| **completeness** | Does the change address the whole requirement, including failure paths? |
| **best-practice** | Does it honor the project's own architecture rules (cite the rule)? |
| **extensibility** | Will the next change here be cheap — without premature abstraction? |
| **logic** | Is behavior correct on the paths tests did not exercise? |
| **naming** | Do new names match the project's recorded vocabulary? |
| **root-cause** | Does the fix remove the cause, or hide the symptom? |

## Lens file contract

Each `.claude/lenses/<id>.md` carries frontmatter:

```yaml
---
id: <kebab-case>
applies_to: [code, design, spec, plan]
anchors:
  - constitution   # rule(s) this lens cites as authority; constitution is
                   # always present. Add project rules during install.
---
```

Body: a high-signal question, optionally with a few clarifying facets —
never a linter-style exhaustive checklist. Findings reference an anchor
(rule slug, not a file path) as the authority — no finding without a
citation. On install, re-point or add anchors to the project's actual
rules where they exist.

## Severity routing

Severity is review PRIORITY, never a license to auto-edit a subjective finding.

| Severity | Action | Owner |
|---|---|---|
| Critical | Propose fix, apply on approval | Operator approves |
| Blocker | Propose fix, apply on approval | Operator approves |
| Major | Report, operator decides | Operator |
| Minor | Report as advisory | Informational |
