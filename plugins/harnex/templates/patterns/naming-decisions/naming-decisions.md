---
paths:
  - "**/*.py"
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.rs"
---

# Naming decisions

Team-level naming conventions that the formatter does not enforce and the
model cannot infer from context alone. Record decisions here so all
developers (human and AI) produce consistent names.

## File naming

<!-- Fill in: kebab-case? snake_case? PascalCase for components? -->

## Tool / script suffixes

<!-- Fill in: e.g., *-lint, *-audit, *-build, *-sync, *-check.
     A closed suffix enum prevents ad-hoc names. -->

## Parameter bag suffixes

<!-- Fill in: when to use Config vs Options vs Params vs Settings.
     Example: Config = immutable after init, Options = per-call overrides. -->

## Factory / constructor verbs

<!-- Fill in: create* = allocates, define* = registers, from* = converts.
     Consistent verb choices prevent "make vs build vs create" drift. -->

## Domain vocabulary

<!-- Fill in: the project's ubiquitous language. Example:
     - "tenant" (not "organization" or "workspace")
     - "pipeline" (not "workflow" or "flow")
     - "source" (not "provider" or "connector") -->

<!-- Every section is a team decision. If a section doesn't apply,
     delete it. The value is having the decision RECORDED, not the
     specific choice. -->
