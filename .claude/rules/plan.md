---
paths:
  - "crates/harness-core/src/plan.rs"
  - "crates/harness-cli/src/commands/plan.rs"
  - "plugins/harnex/templates/patterns/spec-workflow/**"
---

# plan — the spec-workflow review grammar's computer

`harness_core::plan` owns the grammar the spec-workflow templates write:
finding rows under `## Outstanding issues`, decision bullets under
`## Decision log`, the `<n>C/<n>B/<n>M/<n>m` counts, terminal dispositions
(`Disposition::ALL`), and the `acknowledged:` escalation hatch. The template
prose is a projection of these constants — `pattern_manifest_sync` holds the
disposition spelling, the gates.md example line, and `REVIEW_CLASS_GATES`
in lock-step. Change the grammar in the module and let the failing tests name
every prose site.

- No config load in `plan audit`: files are named by the caller, the grammar
  is harness vocabulary. Where specs live is project vocabulary and never
  enters this crate (constitution VII).
- No git in the module: the baseline is text the caller supplies. The shipped
  `check-plan.sh` arm pipes `git show` into it, and `plan_template_sync`
  (harness-cli) holds the flags that arm spells to the clap surface.
- Gate names stay open. Only the two `REVIEW_CLASS_GATES` owe counts; any
  gate that writes counts opts into the convergence comparison.
- A finding-shaped list item that does not parse is a Major finding, never a
  silently skipped row. Keep the detector wider than the parser on every axis
  (marker, case, decoration) — narrowing it restores the silence.
- Unreadable is never empty. A duplicate heading or an unclosed fence is its
  own Blocker; a missing section is a Major, not a pass.
- Vanish semantics: every open baseline row survives verbatim (whitespace
  collapsed) at its rank, or carries a disposition. Rewording is deletion;
  a severity downgrade is deletion.
- The decision log is append-only against its own baseline: committed
  bullets stand verbatim as a prefix of the current log, or
  `plan-log-rewritten` blocks.
