---
paths:
  - "crates/harness-core/src/lifecycle/**"
---

# lifecycle — promotion / retirement / consumer detection

Observation ledger is append-only JSONL per tag. Promoter groups by
`(tag, normalized_text)` where `normalize` lowercases + collapses
whitespace. Candidates require `instance_count ≥ promotion_min_instances`
AND `span ≥ promotion_min_days`.

Retirement classifier emits three signals: Stale (mtime > stale_days),
NoConsumers (grep finds zero), Silent (caller-supplied — derived from
telemetry query). Severity: 3 signals → Major, 2 → Minor, ≤1 → Info.

Exempt sources (`grace_period_days` recency + `[retirement.exempt]` kinds
and slugs) flip `exempt: true` but signals still surface for visibility.

AI never invents decision text. All four decision methods
(`promote` / `reject` / `defer` / `demote`) reject empty `decision_text`
with `LIFECYCLE_DECISION_TEXT_EMPTY`. The CLI mirrors the methods one-to-one
as verb-named subcommands (`harness lifecycle promote|reject|defer|demote`).

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
pattern, and classifies each match. The `Silent` signal is derived
automatically by scanning telemetry payloads for the slug as an exact
string match within the configured `silence_window_days`. Operators
no longer pass `--silent` by hand — `harness lifecycle retire` covers
the entire surface in one call.

When a kind is `foundation: true`, the sweep adds it to `kinds_skipped`
with the reason "foundation kind (excluded from retirement)" — the
exclusion is explicit, never silent.

`harness lifecycle decisions [--tag T] [--decision D]` lists every
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
