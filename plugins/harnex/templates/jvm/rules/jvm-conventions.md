---
paths:
  - "**/*.java"
  - "**/*.kt"
  - "**/*.kts"
---

# JVM conventions

Project-specific decisions the formatter and the compiler do not enforce.
Style lives in the formatter (google-java-format / ktlint) — never restate
here. Scaffold fills each section from the codebase it observes; the entries
below are common defaults to keep only if they match the project's actual
practice.

## Errors

- <!-- harnex-fill: exception discipline in use — checked exceptions, a sealed
  result type, Kotlin `Result`, a library such as Arrow or Vavr -->
- Common default: unchecked exceptions carry a message naming the input that
  failed; a caller that can act on the failure gets a typed exception rather
  than a boolean or a null return.

## Nullability

- <!-- harnex-fill: how absence is expressed — `Optional`, `@Nullable`/`@NonNull`
  annotations, Kotlin's own null types, none -->
- Common default: absence is expressed in the type, not by a documented
  `null`. In mixed Java/Kotlin code, annotate the Java side so Kotlin sees a
  platform type as nullable rather than inferring a non-null it cannot hold.

## Testing

- <!-- harnex-fill: framework and assertion library — JUnit 5, JUnit 4, Kotest,
  Spock; AssertJ, Truth, Hamcrest, kotlin.test -->
- Common default: one behaviour per test, named for the behaviour rather
  than the method under test. Fixtures build through the project's existing
  builder or factory rather than a new one per test class.

## Dependencies

- <!-- harnex-fill: how versions are declared — Gradle version catalog
  (`gradle/libs.versions.toml`), a BOM / dependency-management block,
  inline coordinates -->
- Common default: versions are declared in one place and referenced by
  alias; a module never pins a version its aggregator already fixes.

<!-- Replace every `harnex-fill` marker with what this codebase actually
     does AND where that is declared, as a backtick path — `ruff` —
     `pyproject.toml`. A convention with no named owner is prose that drifts
     the day someone changes the tool, and nothing catches it; a pointer can be
     checked by a reader, and `harness check` resolves the `path.ext:line` form
     against the tree. Both harnesses this template is modelled on name an
     owner in every rule they carry.

     A section with no signal yet takes an explicit "none observed yet" plus
     the default that applies until one appears. -->
