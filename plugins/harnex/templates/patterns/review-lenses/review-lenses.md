---
paths:
  - ".claude/lenses/**"
  - ".claude/skills/**"
---

# Review lens framework

A convergent review loop walks every registered lens over a change set,
ranks findings by severity, proposes fixes for the high-severity ones, and
re-walks the (possibly grown) scope until convergence or a stall limit.


## Where the procedure lives

Running the loop is a workflow, so it is a skill: `.claude/skills/review/`.
This file is the vocabulary that skill judges by — the lenses, the severities,
and the one rule that decides what may be fixed without asking.

## Severity is priority. The citation decides what gets fixed.

A lens finding is a JUDGMENT, not a mechanical check — a lens calls "premature
abstraction" or "wrong name" by reasoning. Per keep-soften-cut, a prose
judgment must never drive a silent edit. So severity ranks attention, and a
second axis decides authority:

| Severity | citing a re-runnable authority | citing judgment |
|---|---|---|
| `Critical` | the loop may fix it | reported, never fixed |
| `Blocker` | the loop may fix it | reported, never fixed |
| `Major` | reported, never fixed | reported, never fixed |
| `Minor` | reported, never fixed | reported, never fixed |

A **re-runnable authority** is something other than the loop's own opinion that
confirms the result: a `.claude/rules/*.md` slug, a lint or type-check code, a
named test. <!-- harnex-fill: this project's other re-runnable authorities — a
schema, a codegen check, a named CI gate -->

`judgment` is the author's own opt-out into the right column, and using it is
never a weaker finding — it is an honest one.

## Filing discipline

- **Coverage precedes verdict.** Name what was read before saying what was
  found. A zero-finding pass with its coverage shown is a complete result; the
  same words without it are an assertion.
- **An absence is claimed from the read, not from a search that missed.** A
  search returning nothing is evidence about the search term.
- **Refute before filing.** Ground truth that contradicts a finding drops it.
  An attempt that settles nothing DOWN-CALIBRATES it — keep it, note what
  blocked the check, cap it at Major. Critical and Blocker are the tier that
  edits files and stops a gate; neither may rest on a claim the pass could not
  establish. Dropping it instead files a verdict nobody reached, silently,
  which is the half that costs.

## Finding format

```
- **[<severity>]** path:line — <what is wrong> [<rule-slug>|<lint-code>|<test>|judgment]
```

## Default lenses

Six lenses ship as the baseline review vocabulary. Each leads with a
high-signal review question and may add a few clarifying facets — never a
linter-style exhaustive checklist or a list of model-default checks (those
belong to the formatter, type checker, and the model's own defaults, per
keep-soften-cut). Add, remove, or customize lenses to match your project's
priorities.

| Lens | High-signal question |
|---|---|
| **completeness** | Does the change address the whole requirement, including failure paths? |
| **best-practice** | Does it honor the project's own architecture rules (cite the rule)? |
| **extensibility** | Will the next change here be cheap — without premature abstraction? |
| **logic** | Is behavior correct on the paths tests did not exercise? |
| **naming** | Do new names match the project's recorded vocabulary? |
| **root-cause** | Does the fix remove the cause, or hide the symptom? |

## Lens file contract

Each `.claude/lenses/<id>.md` carries frontmatter:

```yaml
---
id: <kebab-case>
applies_to: [code, prose]
anchors:
  - constitution   # rule(s) this lens cites as authority; constitution is
                   # always present. Add project rules during install.
---
```

`applies_to` is a closed vocabulary, and the loop skips a lens on a file the
lens does not claim — so a token nobody defines silently scopes a lens to
nothing:

| Token | The files it covers |
|---|---|
| `code` | source and its tests — whatever this project's formatter and type checker run over |
| `prose` | the documentation beside code: `CLAUDE.md`, `.claude/rules/*.md`, package docs. The loop pulls these in as a code file's sibling, so a lens that omits `prose` cannot see the stale-paragraph finding that pairing exists to surface |
| `spec` | a spec or design document under `specs/` or an ADR directory |
| `plan` | the implementation plan of a spec, where one exists |

<!-- harnex-fill: any file class this project reviews that these four do not
     name — a schema, a migration, an infrastructure definition -->

A lens claiming every token is not thereby thorough; it is unscoped, and the
loop will walk it somewhere it has nothing to say.

Body: a high-signal question, optionally with a few clarifying facets —
never a linter-style exhaustive checklist. Findings reference an anchor
(rule slug, not a file path) as the authority — no finding without a
citation. On install, re-point or add anchors to the project's actual
rules where they exist.
