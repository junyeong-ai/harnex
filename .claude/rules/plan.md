---
paths:
  - "crates/harness-core/src/plan.rs"
  - "crates/harness-cli/src/commands/plan.rs"
  - "plugins/harnex/templates/patterns/spec-workflow/**"
governs:
  concept: the spec-workflow review grammar computer
  live_truth:
    - crates/harness-core/src/plan.rs
---

# plan — the spec-workflow review grammar's computer

`harness_core::plan` owns the grammar the spec-workflow templates write:
finding rows under `## Outstanding issues`, decision bullets under
`## Decision log`, the per-class counts tokens, terminal dispositions
(`Disposition::ALL`), and the per-gate round budget. The template
prose is a projection of these constants — `pattern_manifest_sync` holds the
disposition spelling, the gates.md example line, and `COUNTED_GATES`
in lock-step. Change the grammar in the module and let the failing tests name
every prose site.

- No config load in `plan audit`: files are named by the caller, the grammar
  is harness vocabulary. Where specs live is project vocabulary and never
  enters this crate (constitution VII).
- No git in the module: the baseline is text the caller supplies. The shipped
  `check-plan.sh` arm pipes `git show` into it, and `plan_template_sync`
  (harness-cli) holds the flags that arm spells to the clap surface.
- Gate names stay open, and the caller declares the set its workflow has: the
  budget is keyed by the name on the line, so a firing under a name nothing
  declares is a budget of its own and `plan-log-gate-undeclared` blocks it.
  Undeclared, no name is held — the flag is opt-in like the budget it guards.
  Only `COUNTED_GATES` owe counts, each in its declared
  `GateClass`: review gates count findings by rank, `acceptance` counts
  criteria by outcome. The approval rule reads `GateCounts::blocking` and
  never the class — unmeasured blocks an acceptance approval exactly as a
  Blocker blocks a review's, because a criterion nothing answered is not one
  that passed. A firing carrying the other class's token is its own finding:
  the wrong token parses and reports a total the gate does not owe.
- The round budget is the only convergence control. It counts `needs_revision`
  firings per gate over the spec's life and nothing returns one — the loop the
  budget bounds writes every word of this log, so any token that could lower
  the count is a budget the loop hands itself. Never add a round-to-round
  comparison, a rationale hatch, or a reset. The crossing is reported against
  the record that appended it, not against the log's history: monotone, a
  history-held finding blocks the wrapup commits that are the way out of a
  budget. Closing the gate spends nothing and returns nothing, because the
  operator's exit at the budget has to be writable at the budget.
- A finding-shaped list item that does not parse is a Major finding, never a
  silently skipped row. Keep the detector wider than the parser on every axis
  (marker, case, decoration) — narrowing it restores the silence. Position is
  the axis the log adds: an entry nested, indented or quoted off the margin
  renders as a firing and reaches no parser, so `plan-log-off-margin` reports
  it. Decide it by the line an entry opens on — a rationale quoting the
  grammar is one entry, and reading mentions makes a quotation a finding.
- Unreadable is never empty. A duplicate heading or an unclosed fence is its
  own Blocker; a missing section is a Major, not a pass.
- Vanish semantics: every open baseline row survives verbatim (whitespace
  collapsed) at its rank, or carries a disposition. Rewording is deletion;
  a severity downgrade is deletion.
- The decision log is append-only against its own baseline: committed
  bullets stand verbatim as a prefix of the current log, or
  `plan-log-rewritten` blocks.
- A commit that adds rows records the round that found them: rows past the
  committed plan and no round past the committed log is
  `plan-round-unrecorded`. A round is what the budget counts and nothing else
  — `needs_revision` under a gate that carries one — so an approval, a
  deferral, a rejection and a gate outside `COUNTED_GATES` record none, and
  the seam and the budget cannot disagree about what a round is. New is keyed
  by `FindingRow::identity`, the vanish check's own key, and a committed row
  that did not survive explains one unclaimed row and no more: the surplus is
  what no rewording accounts for, which is what the finding counts.
- A committed section that does not enumerate is `plan-baseline-unreadable`,
  not three silences. Every check that reads a baseline answers nothing
  without one, and a verdict read off a section nobody enumerated would be a
  finding invented from silence. Absent is not unreadable: a document
  committed to nothing held no rows and no records, which is the first commit
  of a spec.
