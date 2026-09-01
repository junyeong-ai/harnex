# Enforced vs advisory (the organizing principle)

The single axis that decides where a guardrail belongs. Quality scales across
many different developers only through the enforced layer — it is the only
thing that survives a confused, careless, or adversarial agent turn.

## Enforced — deterministic, non-bypassable by the model

| Surface | Why it holds |
|---|---|
| **Hooks** (PreToolUse / PermissionRequest exit 2 or `permissionDecision: deny`) | Run as the client at lifecycle events "regardless of what Claude decides." The only block that a reasoning model cannot talk itself past. |
| **`permissions.deny` / `ask` / `allow`** | Client-enforced; deny wins, first match, merges across scopes. |
| **Managed settings** | Highest precedence, cannot be overridden; org floors (`allowManagedPermissionRulesOnly`, `disableAllHooks`, `strictPluginOnlyCustomization`). |
| **Sandbox** | Filesystem/network isolation for Bash. |

## Advisory — shapes behavior, no guarantee

| Surface | Reality |
|---|---|
| **CLAUDE.md / `.claude/rules/`** | Delivered as a user message after the system prompt — "no guarantee of strict compliance." |
| **Skills** | Instructions + `allowed-tools` pre-approval (grants, not restricts). |
| **Auto-memory** | Model-written notes. |

## The rule harnex generates by

1. **Must-happen → enforced.** Anything that must occur at a point in the
   loop (format-on-edit, block `rm -rf` / secret read, scan before commit)
   becomes a hook or a `permissions.deny` rule — never a CLAUDE.md sentence.
   The memory doc itself redirects: "write it as a hook instead."
2. **Guidance → advisory, minimal, path-scoped.** Architecture intent,
   conventions, where-things-live go in short CLAUDE.md + `.claude/rules/*.md`
   with `paths:` frontmatter so each developer's context stays lean.
3. **Workflow → skill.** Repeatable multi-step procedures (the harnex modes
   themselves) are skills: description costs ~nothing until invoked;
   `disable-model-invocation: true` for side-effectful flows.
4. **A declared control names its computer.** A cap, floor, threshold, or
   convergence criterion stated in prose is an intention until something
   computes it — a hook, a test, a validator, or a recorded count the next
   step must read before proceeding. State the computer beside the control;
   one with no computer is an observation for the lifecycle ledger, not a
   rule. The measured failure shape: a review gate whose "stop when the
   count does not fall" lived in prose while no round count was recorded —
   it ran eleven rounds, and clearing findings was indistinguishable from
   narrating them away. The spec-workflow's own controls name theirs:
   `harnex plan audit` computes the counts comparison, the disposition
   floor, and the append-only row contract, and the shipped pre-commit arm
   holds them at the commit.

## Unmeasured is not passed

A gate reports what it measured, and the one report it may never emit is the
shape of a pass over something it did not read. Zero findings across a corpus
the gate never opened is byte-identical to zero findings across one it read
closely, so the reader cannot tell them apart and reads the stronger claim.

Give the third answer a name in the type rather than a caveat in the prose
around it. Every enforced surface harnex ships names its own:

| Surface | The zero it refuses | What it says instead |
|---|---|---|
| `lifecycle retire` | silence inferred from a record that could never have named the artifact | `unmeasured`, which fires no signal |
| `plan audit` acceptance | a criterion nothing answered, counted as passed | `unmeasured`, blocking an approval exactly as a Blocker does |
| `hooks/pre-commit` | a secret scan that failed, exiting like one that found nothing | a code of its own for findings, so a failure is unscanned rather than clean |
| `lifecycle candidates` | an unwritten or unreadable ledger, read as a corpus that produced nothing | `observations_read` beside the candidates, and an error where the read failed |
| `guard stop-audit` | a probe that gave no answer, read as "there is work" | a skip naming why, having spent nothing |

Two shapes produce it, and a generated gate is worth checking against both. A
**default standing in for a measurement** — an empty configuration read as
"yes", an absent record read as zero — decides on the reader's behalf and
tells them nothing. A **read that came back short** — a directory absent for
some reason other than emptiness, a record the scan's own filter skipped —
shortens the result instead of failing, so the count is smaller than the
corpus and nothing marks the difference.

## The advisory class — freshness gates, findings never do

Between the two tiers sits the measurement neither can hold: expensive,
stochastic, or judgment-shaped (a contrast audit, a behaviour eval, a
performance snapshot). Gating on its findings absorbs its flakiness into the
gate; leaving it advisory prose lets it silently rot. The resolution inverts
what gates: **the advisory never blocks — the freshness of its basis does.**

- The project's own instrument measures; `harnex evidence record --id <id>`
  writes the baseline with content digests of the declared inputs and of the
  instrument itself. The residual is stated, not implied: only in-tree
  content is identity — an engine that shells out to an npm or system
  binary re-points with no tree diff, so declare its lockfile or version
  pin beside it, and an out-of-tree instrument is outside this basis
  entirely.
- `harnex check` asks only "does the evidence still describe its inputs" —
  a moved digest, a moved instrument, a moved declaration, or no recording
  at all (`advisory-unmeasured` — never a fabricated zero) is the finding.
  It never asks "did it get worse"; the payload is the project's to judge.
- `--unattended` (a push gate, CI) gates staleness only where the entry
  declares `unattended_remeasure` — a push gate may only block on drift the
  pusher can clear in the same sitting.
- An advisory's own findings enter reports with a disposition, not a
  severity: `mention` (say it where the operator reads) or `route` (hand it
  to its named reader). A disposition is not a soft severity — it names the
  reader instead of ranking the alarm. This is report vocabulary the prose
  holds; nothing in the oracle reads the payload, by design.

Declared under `[[evidence.advisories]]`; the baseline is a committed
`evidence/<id>.json` the schema of which is closed.

## What this means for a generated harness

- The enforced tier is the part that genuinely equalizes quality across a
  team of vibe-coders. Generate it correctly and language-appropriately
  (formatter hook, secret-deny block, destructive-op deny, hook-wrapper
  routing) — this is the highest-leverage output.
- Do not encode an enforcement intent as advisory prose and call it done. A
  "always run lint before committing" line in CLAUDE.md is not enforcement; a
  pre-commit hook is.
- Do not over-fill the advisory tier (see keep-soften-cut.md): every line is a
  recurring per-session token cost and a context-rot contributor.
