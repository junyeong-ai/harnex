---
paths:
  - "**/*.ts"
  - "**/*.tsx"
---

# TypeScript conventions

Project-specific decisions that the language tooling does not enforce.
Style lives in biome — never restate here.

## Module shape

- Public surface: a barrel `index.ts` per package re-exports the typed API.
  Internal helpers stay unexported.
- Cross-package imports go through the barrel, never through deep paths.

## Async boundaries

- Library code returns `Promise<T>`; never start a fire-and-forget task at
  module load. A subscription that owns its own lifecycle goes behind a
  `start()` / `stop()` pair.

## Errors

- Throw typed `Error` subclasses; never bare strings. Subclasses live in
  the same module as the surface that raises them.
