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
   (default 3 iterations — configurable per invocation).

## Lens file contract

Each `.claude/lenses/<id>.md` carries frontmatter:

```yaml
---
id: <kebab-case>
applies_to: [code, design, spec, plan]
anchors:
  - <rule-slug>:<concept> cited as authority
---
```

Body: the evaluation criteria this lens checks. Findings reference an
anchor (rule + concept) as the authority — no finding without a citation.

## Severity routing

| Severity | Action | Owner |
|---|---|---|
| Critical | Auto-fix immediately | Review skill |
| Blocker | Auto-fix immediately | Review skill |
| Major | Report, operator decides | Operator |
| Minor | Report as advisory | Informational |

<!-- Fill in your project's lenses under .claude/lenses/. Common starting
     axes: completeness, best-practice, extensibility, logic, naming,
     root-cause. Each lens is a separate file. -->
