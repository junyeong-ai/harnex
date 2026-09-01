---
paths:
  - "crates/harness-core/src/lifecycle/**"
governs:
  concept: the observation and decision ledgers
  live_truth:
    - crates/harness-core/src/lifecycle
---

# lifecycle — promotion / retirement / consumer detection

Observation ledger is append-only JSONL per tag. Promoter groups by
`(tag, normalized_text)` where `normalize` lowercases + collapses
whitespace. Candidates require `instance_count ≥ promotion_min_instances`
AND `span ≥ promotion_min_days`.

`survey` answers with the candidates AND the corpus they were drawn from —
`observations_read`, `decisions_read`, `groups_considered`, `groups_resolved`
— because a pass that drains the ledgers reads an unwritten one and a corpus
that produced nothing oppositely. It reads two ledgers and reports both: a
decision ledger that was not found resolves nothing, which is what a pass that
has settled nothing also looks like. The counts close: every observation falls
in exactly one group, every group is either resolved by a suppressing decision
or considered against the thresholds. Sort candidates by instance count, then
`(tag, normalized_text)`: groups arrive in hash order and one ledger owes one
envelope.

Both ledgers file by tag and scan by the `.jsonl` filename suffix, never
`Path::extension`, which answers `None` for a leading-dot name. Refuse an
empty tag at the encoder both appends route through: its stem files a record
as `.jsonl`, and what a ledger accepts is what it must read back. Absent means
nothing written; a path that fails to stat and a directory symlink with no
target are reads that did not happen, and each fails.

Retirement classifier emits three signals: Stale (mtime > stale_days),
NoConsumers (grep finds zero), Silent (caller-supplied `SilenceState` —
derived from the telemetry query). Severity: 3 signals → Major, 2 → Minor,
≤1 → Info. `SilenceState` is tri-state — `Silent` fires the signal;
`Active` and `Unmeasured` do not. Silence is measured only against
`[[kinds]] invocation_kind`, the telemetry Kind that kind declares as the
record of its artifacts' invocations. Undeclared, or with no event of it in
the window, that kind's slugs are `Unmeasured` and cap severity at Minor.
Silence is a claim about invocations, so neither half is inferred: not which
Kind is the record — an unrelated payload may carry any string, and reading
one as an invocation decides a slug's fate on a coincidence — and not which
artifacts it speaks for, since a record naming skills can never name a rule,
which is loaded rather than invoked.

Exempt sources (`grace_period_days` recency + `[retirement.exempt]` kinds
and slugs) flip `exempt: true` but signals still surface for visibility.

AI never invents decision text. All four decision methods
(`promote` / `reject` / `defer` / `demote`) reject empty `decision_text`
with `LIFECYCLE_DECISION_TEXT_EMPTY`. The CLI mirrors the methods one-to-one
as verb-named subcommands (`harnex lifecycle promote|reject|defer|demote`).

Decision-to-surfacing mapping (via `PromotionDecision::suppresses_resurfacing`):
- `Approved` → suppresses
- `Rejected` → suppresses
- `Demoted` → suppresses
- `Deferred` → keeps surfacing (informational)

`demote` requires the LATEST decision for the same
`(tag, normalized_text)` pair to be `Approved` — refused with
`LIFECYCLE_DEMOTE_WITHOUT_APPROVAL` otherwise. A second `demote`
without an intervening re-Approval is refused (no Approved state to
retract from). Rehab path Approved → Demoted → re-Approved → Demoted
is supported. All records persist append-only; the suppression set
treats every `Approved | Rejected | Demoted` ledger entry as terminal.

`RetirementSweeper` is the top-level retirement runner. It walks every
`[[kinds]]` declaration (skipping `foundation = true` kinds), finds
the matching `[[lifecycle.consumer_detectors]]`, globs the kind's path
pattern, and classifies each match. The silence state is derived from one
scan of the declared `invocation_kind` records within `silence_window_days`,
matching each slug as an exact string in a payload; a kind declaring no record,
or one whose record the window does not hold, yields `Unmeasured` — never a
fabricated `Silent`. Operators
`harnex lifecycle retire` covers the entire surface in one call.

When a kind is `foundation: true`, the sweep adds it to `kinds_skipped`
with the reason "foundation kind (excluded from retirement)" — the
exclusion is explicit, never silent.

`harnex lifecycle decisions [--tag T] [--decision D]` lists every
record in the decision ledger sorted by timestamp descending. Operators
audit the promote / reject / defer / demote history without reading
raw jsonl.

ConsumerDetector is a trait; built-in strategies are the `ConsumerStrategy`
variants (that enum is the source of truth — do not count them here):
- `grep` — walks working tree, matches `{slug}`-substituted pattern.
- `graph-backlinks` — calls `nodex query backlinks <node_id>` via the
  graph module. Fails explicitly if nodex is absent (never silent
  fallback to grep). Pattern field holds the node-id template.

When adding a new strategy: add a `ConsumerStrategy` variant (single
source of truth — `from_str`/`as_str`/`ALL` derive from it), add a
`ConsumerDetector` impl, add a match arm in `consumer_detector_for`
(exhaustive match enforces this step at compile time), add a test
asserting both happy and unknown-strategy paths.
