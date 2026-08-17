---
paths:
  - "**/*.ts"
  - "**/*.tsx"
---

# TypeScript conventions

Project-specific decisions that the language tooling does not enforce. Style
lives in biome — never restate here. Scaffold fills each section from the
codebase it observes; the entries below are common defaults to keep only if
they match the project's actual practice.

## Module surface

- <!-- harnex-fill: export pattern — barrel `index.ts`, direct deep imports, mixed -->
- Common default: a barrel `index.ts` per package re-exports the public API;
  cross-package imports go through it. Replace if the project deliberately
  uses deep imports (some monorepos do for tree-shaking).

## Async boundaries

- <!-- harnex-fill: async pattern in existing code -->
- Common default: library code returns `Promise<T>`; no fire-and-forget task
  at module load — a self-owned subscription sits behind `start()`/`stop()`.

## Errors

- <!-- harnex-fill: error pattern — typed Error subclasses, Result type, none -->
- Common default: throw typed `Error` subclasses defined in the module that
  raises them; never bare strings.

<!-- Replace every `harnex-fill` marker with what this codebase actually
     does AND where that is declared, as a backtick path — `ruff` —
     `pyproject.toml`. A convention with no named owner is prose that drifts
     the day someone changes the tool, and nothing catches it; a pointer can be
     checked by a reader, and `harness check` resolves the `path.ext:line` form
     against the tree. Both harnesses this template is modelled on name an
     owner in every rule they carry.

     A section with no signal yet takes an explicit "none observed yet" plus
     the default that applies until one appears. -->
