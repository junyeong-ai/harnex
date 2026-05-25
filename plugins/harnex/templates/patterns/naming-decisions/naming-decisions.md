---
paths:
  - "**/*.py"
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.rs"
---

# Naming decisions

Team-level naming conventions that the formatter does not enforce and the
model cannot infer from context alone. Defaults below are drawn from two
independently-converged production harnesses; override where they conflict
with your team's existing practice.

## File naming

- Source files: `kebab-case` (TypeScript/web) or `snake_case` (Python/Rust).
- Test files: co-located as `<name>.test.ts` or `test_<name>.py`.
- Config files: `<purpose>.config.{ts,json,toml}` (never bare `config`).

## Tool / script suffixes

A closed suffix set prevents ad-hoc naming across the toolchain:

| Suffix | Meaning | Example |
|---|---|---|
| `-lint` | Static analysis, no mutation | `harness-lint` |
| `-audit` | Cross-input semantic check, no mutation | `dep-audit` |
| `-check` | Structural validation, may exit non-zero | `type-check` |
| `-build` | Produces an artifact | `docker-build` |
| `-sync` | Synchronizes two representations | `schema-sync` |
| `-format` | Rewrites files for style | `code-format` |

## Factory / constructor verbs

| Verb | Meaning |
|---|---|
| `create*` | Allocates a new resource (may have side effects) |
| `build*` | Assembles from parts (pure, returns value) |
| `from*` | Converts from another representation |
| `parse*` | Deserializes from string/bytes |
| `define*` | Registers a definition (declarative, not imperative) |

## Parameter bag suffixes

| Suffix | Semantics |
|---|---|
| `Config` | Immutable after initialization; read-only at runtime |
| `Options` | Per-call overrides; merged with defaults |
| `Params` | Positional/required arguments (not optional) |

## Domain vocabulary

<!-- Fill in: your project's ubiquitous language. Example:
     - "tenant" (not "organization", "workspace", or "account")
     - "pipeline" (not "workflow", "flow", or "chain")
     Consistent vocabulary prevents the same concept having three names
     across the codebase. -->
