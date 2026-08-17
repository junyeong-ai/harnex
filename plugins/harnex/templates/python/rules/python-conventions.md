---
paths:
  - "**/*.py"
---

# Python conventions

Project-specific decisions that the language tooling does not enforce. Style
lives in ruff — never restate here. Scaffold fills each section from the
codebase it observes; the entries below are common defaults to keep only if
they match the project's actual practice.

## Typing

- <!-- harnex-fill: type checker in use — mypy, ty, pyright, none -->
- Common default: public functions carry full annotations; the type checker
  runs in the gate.

## Serialization

- <!-- harnex-fill: serialization lib — Pydantic, attrs, dataclasses, msgspec -->
- Common default: one validation library at wire boundaries; plain
  dataclasses for internal value types. Replace with the project's choice —
  never mix two for the same shape.

## Errors

- <!-- harnex-fill: error pattern — typed Exception subclasses, error codes, none -->
- Common default: raise typed `Exception` subclasses defined alongside the
  surface that raises them; never bare `Exception` from library code.

## Module shape

- <!-- harnex-fill: public-API convention in existing `__init__.py` files -->
- Common default: package `__init__.py` exposes the stable API; no
  side-effecting imports (registration behind an explicit `init()`).

<!-- Replace every `harnex-fill` marker with what this codebase actually
     does. A section with no signal yet takes an explicit "none observed yet"
     plus the default that applies until one appears. -->
