---
paths:
  - ".claude/lenses/**"
  - ".claude/skills/**"
---

# Review lens framework

A convergent review loop walks every registered lens over a change set,
partitions findings by severity, auto-fixes Critical and Blocker findings,
and re-walks the (possibly grown) scope until convergence or a stall limit.

## Loop semantics

1. Walk every lens in `.claude/lenses/` over the input scope.
2. Partition findings: Critical/Blocker → auto-fix; Major/Minor → report.
3. Re-walk the scope (may have grown from auto-fix edits).
4. Stop when 0 Critical + 0 Blocker remain, OR stall limit reached
   (default 3 iterations).

## Default lenses

Six lenses ship as the baseline review vocabulary. Each is a high-signal
review question — not a checklist of linter-owned or model-default checks
(those belong to the formatter, type checker, and the model's own defaults,
per keep-soften-cut). Add, remove, or customize lenses to match your
project's priorities.

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

Body: a high-signal question, not a checklist. Findings reference an anchor
(rule slug) as the authority — no finding without a citation. On install,
re-point or add anchors to the project's actual rules where they exist.

## Severity routing

| Severity | Action | Owner |
|---|---|---|
| Critical | Auto-fix immediately | Review skill |
| Blocker | Auto-fix immediately | Review skill |
| Major | Report, operator decides | Operator |
| Minor | Report as advisory | Informational |
