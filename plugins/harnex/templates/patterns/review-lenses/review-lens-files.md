---
paths:
  - ".claude/lenses/**"
  - ".claude/skills/review/**"
  - ".claude/skills/critique/**"
  - ".claude/skills/design-review/**"
governs:
  concept: where the review procedures live and what a lens file declares
  live_truth:
    - .claude/lenses
    - .claude/skills/review
    - .claude/skills/critique
    - .claude/skills/design-review
    - .claude/agents/reviewer.md
---

# Review lens files

What a review judges by is `.claude/rules/review-lenses.md`, which loads in every
session because judgment answers to no file. This is the other half: which file holds
which procedure, and what a lens file must declare to be walked.

## Where each procedure lives

- `.claude/skills/review/SKILL.md` — the mutating loop. Edits files, closes
  with its own fresh-context terminal pass.
- `.claude/skills/critique/SKILL.md` — one forked, read-only lens walk over a
  change set. Findings are its only output.
- `.claude/skills/design-review/SKILL.md` — forked refutation of a design
  document before code exists, self-gated on the trigger below.
- `.claude/agents/reviewer.md` — the fresh context every forked pass runs as.

This file is the vocabulary all of them judge by — the severities, what a
finding blocks, the authorities, the two refutation regimes and the witness
both require, the trigger, and the lens contract.

## Lens file contract

Each `.claude/lenses/<id>.md` carries frontmatter:

```yaml
---
id: <kebab-case>
applies_to: [code, prose]
anchors:
  - rule:constitution   # authorities this lens cites, as <source>:<id> per
                        # the column above. Add project rules during install.
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
never a linter-style exhaustive checklist. Findings reference an anchor's
bare `<id>` (an authority id per the column above, never a file path) — no
finding without a citation. On install, re-point or add anchors to the
project's actual rules where they exist.
