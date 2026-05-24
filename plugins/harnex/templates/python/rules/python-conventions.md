---
paths:
  - "**/*.py"
---

# Python conventions

Project-specific decisions that the language tooling does not enforce.
Style lives in ruff — never restate here.

## Module shape

- Each package's `__init__.py` exposes a stable public API. Internal
  modules import via the package root, not deep paths.
- Side-effecting imports (registering hooks, mutating globals on import)
  are forbidden — put them behind an explicit `init()` call.

## Typing

- Public functions carry full type annotations checked by `ty`.
- Pydantic models for serialization boundaries; dataclasses for internal
  value types. Never both for the same shape.

## Errors

- Raise typed `Exception` subclasses defined alongside the surface that
  raises them. Never raise bare `Exception` from library code.
