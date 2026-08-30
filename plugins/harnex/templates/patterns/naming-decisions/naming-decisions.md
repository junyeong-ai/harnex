---
paths:
  - "**/*.py"
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.rs"
---

# Naming decisions

The vocabulary this codebase already uses, written down so it stays one
vocabulary. Formatters do not enforce naming and a model cannot infer a
convention from a file it has not opened, so the same concept acquires three
names and the same suffix three meanings.

Every table below is **read out of this repository, not chosen for it**. A
convention imported from somewhere else is worse than none: it contradicts the
code a reader is looking at, and the code wins. Where a section has no
established practice yet, say so and pick one — that is a decision, and it
belongs in a commit body as well as here.

Name the file that settles each answer. A table with no owner drifts the first
time someone renames a directory; a pointer can be checked, and `harnex check`
resolves a marked claim against the tree — the `file:` marker in square
brackets, holding a project-relative path and optionally a `:line`. Write it
around each owner named below; the marker is a reserved token, so an example of
the syntax belongs in a fenced block or a comment, never in prose.

## File naming

<!-- harnex-fill: the dominant casing per file kind, counted across the tree —
     source, test, config — with a representative path for each -->

## Tool / script suffixes

A closed suffix set is what stops the next script inventing a seventh word for
"checks something". Fill from the scripts that exist; leave a row out rather
than inventing a meaning nobody has used.

| Suffix | Meaning in this repo | Example |
|---|---|---|
<!-- harnex-fill: one row per suffix observed across the task runner, CI jobs
     and script directories, each with the real script that establishes it -->

## Factory / constructor verbs

<!-- harnex-fill: the verbs this codebase actually uses to construct things and
     what distinguishes them here — a `create` that allocates versus a `build`
     that assembles is a real distinction only if the code makes it -->

## Parameter bag suffixes

<!-- harnex-fill: the suffixes on option/config types and what each means here,
     or "none observed yet" with the one this repo will use -->

## Domain vocabulary

The ubiquitous language: one word per concept, and the word the code uses.

<!-- harnex-fill: each concept where this repo has settled on one word over
     obvious alternatives, with the module that defines it. Read the type and
     table names, not the prose — "tenant" (not organization/workspace/account)
     is a finding only if the code says tenant. Flag the ones where the code is
     inconsistent: those are the decisions the team still owes. -->
