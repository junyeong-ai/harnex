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

- Observed: <export pattern — barrel `index.ts`, direct deep imports, mixed>.
- Common default: a barrel `index.ts` per package re-exports the public API;
  cross-package imports go through it. Replace if the project deliberately
  uses deep imports (some monorepos do for tree-shaking).

## Async boundaries

- Observed: <async pattern in existing code>.
- Common default: library code returns `Promise<T>`; no fire-and-forget task
  at module load — a self-owned subscription sits behind `start()`/`stop()`.

## Errors

- Observed: <error pattern — typed Error subclasses, Result type, none>.
- Common default: throw typed `Error` subclasses defined in the module that
  raises them; never bare strings.

<!-- Scaffold: detect the real conventions from existing code and replace the
     "Observed:" lines. If a section has no signal yet, keep the default and
     note "none observed yet". -->
