---
paths:
  - ".claude/lenses/**"
  - ".claude/skills/**"
governs:
  concept: the review vocabulary and the skills that judge by it
  live_truth:
    - .claude/lenses
    - .claude/skills/review
    - .claude/skills/critique
    - .claude/skills/design-review
    - .claude/agents/reviewer.md
---

# Review lens framework

A convergent review loop walks every registered lens over a change set, ranks
findings by severity, fixes what a cited authority confirms, and re-walks the
grown scope until convergence. Around it, forked passes restore the one thing
an in-session loop structurally lacks: a context that did not watch the work
form its opinion.

## Where each procedure lives

- `.claude/skills/review/SKILL.md` — the mutating loop. Edits files, closes
  with its own fresh-context terminal pass.
- `.claude/skills/critique/SKILL.md` — one forked, read-only lens walk over a
  change set. Findings are its only output.
- `.claude/skills/design-review/SKILL.md` — forked refutation of a design
  document before code exists, self-gated on the trigger below.
- `.claude/agents/reviewer.md` — the fresh context every forked pass runs as.

This file is the vocabulary all of them judge by — the severities, the
authorities, the two refutation regimes, the trigger, and the lens contract.

## Severity is priority. The citation decides what gets fixed.

A lens finding is a JUDGMENT, not a mechanical check — a lens calls "premature
abstraction" or "wrong name" by reasoning. Per keep-soften-cut, a prose
judgment must never drive a silent edit. So severity ranks attention, and a
second axis decides authority:

| Severity | citing an authority | citing judgment |
|---|---|---|
| `Critical` | the loop may fix it | reported, never fixed |
| `Blocker` | the loop may fix it | reported, never fixed |
| `Major` | reported, never fixed | reported, never fixed |
| `Minor` | reported, never fixed | reported, never fixed |

`judgment` is the author's own opt-out into the right column, and using it is
never a weaker finding — it is an honest one.

## Authorities

An authority is something other than the reviewing context that can confirm a
claim. A lens anchor names one as `<source>:<id>`; a finding cites the bare
`<id>`:

| Source | `<id>` names |
|---|---|
| `rule` | a `.claude/rules/<slug>.md` stem — `rule:constitution` at minimum |
| `lint` | a lint or type-check code the project runs |
| `test` | a named test |
| `gate` | a named CI or pre-commit gate |

<!-- harnex-fill: sources this project's own tooling adds — a schema check, a
     codegen guard, a structural fitness function -->

The column is incomplete and stays that way: `lint` and `test` ids belong to
vocabularies no file here can enumerate, so the boundary degrades toward the
conservative side — a citation naming an authority this file does not know is
surfaced, never rejected and never auto-fixed. A closed check over an
incompletable set would reject a real finding the first time a reviewer
reached for the next vocabulary.

## Two refutation regimes, chosen by subject

Every candidate finding is tested against ground truth before it is filed —
something read or run, never a re-derivation of the claim's own reasoning.
What differs by subject is which direction of error costs more:

- **A code finding feeds an auto-fixer**, so the expensive error is the false
  positive. Ground truth that contradicts the finding drops it. An attempt
  that settles nothing — a race no read can trigger, a branch no fixture
  reaches — **down-calibrates** instead: keep it, note what blocked the
  check, cap it at Major. Critical and Blocker are the tier that edits files
  and stops a gate, and neither may rest on a claim the pass could not
  establish. A failed attempt is evidence about the attempt, exactly as a
  search that misses is evidence about the token — dropping the finding there
  files a verdict nobody reached, silently, which is the half that costs.
- **A design decision is reviewed before code exists**, so the expensive
  error is the false pass — a wrong decision discovered after it is built is
  the costliest correction there is. **Default-refute**: a decision that
  cannot be ground-truthed as sound is a finding, and uncertainty resolves
  toward the finding, never the pass. A refutation that cannot cite an
  authority carries `judgment` and is surfaced for the operator — the valve
  that stops a reviewer from coercing a design toward its own preferences.

The subject is the question asked, not the file extension: a change set under
review takes the first regime even for the spec and prose files inside it,
because an auto-fixer acts on the result; a design not yet built takes the
second. Side by side the two read as contradictory; they are one rule about
asymmetric error cost, applied to two subjects.

## The bookend trigger

A forked pass over a design earns its cost only where the blast radius does.
`design-review` fires on a deterministic disjunction of NAMED signals read
from the change's declared surface — never a tuned score, so two evaluators
reach the same answer:

- `multi-module` — the work spans more than one module or package boundary
- `contract` — a wire contract, schema, public API, or config key moves
- `harness-change` — `.claude/**`, hook scripts, or the gate definitions move
- `rule-tension` — the design itself marks a conflict with a loaded rule

<!-- harnex-fill: this project's own signals, from the enforcer sweep — a
     migration surface, an auth or tenancy path, a generated-file guard -->

The signals read the surface the document itself declares — the files,
modules, and contracts it names as what the work will touch. No signal firing
means the forked pass is skipped and says so — an always-on forked pass over
every change is scaffolding a self-correcting loop does not need. A document
that cannot be read, or that declares no surface at all, fails closed: the
pass fires, because a design that does not say what it touches cannot show
that its blast radius is small.

The mutating loop's own terminal pass is NOT gated on this trigger: an
applied edit is its own blast radius, so verifying one is always warranted —
only a trivial, reversible diff skips it (`.claude/skills/review/SKILL.md`
§ The terminal pass). A pipeline that wants an additional forked confirmation
over a landed diff gates it on this same trigger rather than defining a
second one.

## Filing discipline

- **Coverage precedes verdict.** Name what was read before saying what was
  found. A zero-finding pass with its coverage shown is a complete result; the
  same words without it are an assertion.
- **An absence is claimed from the read, not from a search that missed.** A
  search returning nothing is evidence about the search term.
- **An author's completeness claim is itself a finding to check.** "The
  correction has landed", "the deferred cases are recorded" — each is an
  assertion about work, not the work, and the author is structurally blind to
  the gap between the two. Read the claim against the artifact it describes.
- **Refutation runs before filing**, under the regime the subject selects
  (above).

## Finding format

```
- **[<severity>]** path:line — <what is wrong> [<authority-id>|judgment]
```

The citation is a bare id from the authorities column — any source, its
prefix left to lens anchors — or `judgment`.

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
