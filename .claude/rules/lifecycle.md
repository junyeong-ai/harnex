---
paths:
  - "crates/harness-core/src/lifecycle/**"
governs:
  concept: the observation and decision ledgers
  live_truth:
    - crates/harness-core/src/lifecycle
---

# lifecycle — promotion / retirement / consumer detection

Observation ledger is append-only JSONL per tag. `LedgerReader` groups by
`(tag, normalized_text)` where `normalize` lowercases + collapses
whitespace. Candidates require `instance_count ≥ promotion_min_instances`
AND `span_days ≥ promotion_min_days`.

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

`live` is the survey's complement over the same reading — one `read` of both
ledgers feeds both, so open groups plus closed equals the survey's considered
plus resolved. It lays every group out by tag: tags by breadth (distinct
sources over the tag's open groups) descending then name, open groups by
instance count then text, closed ones as wording plus the latest suppressing
decision, by wording. A tag with nothing open is still listed, because its
closed wordings are what a new sighting is checked against — one recorded
under a closed wording joins that group and surfaces nowhere.
`harnex lifecycle observations [--tag T]` emits it; the filter narrows `tags`
and leaves `observations_read` whole, so an empty `tags` under a written
ledger is nothing under that tag.

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

NoConsumers counts citations, not loads. A path-scoped rule enters context
when the runtime reads a file it governs, and that leaves no citation
anywhere — so zero consumers on a rule says how often its name is written,
never whether it is used. Five of this project's rules classify that way and
none of them is unused. The load question has an owner: `retire.md` answers it
from `facts.harness.rule_loads`, and states the window that evidence needs.

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
with a reason carrying how many paths its glob carved out — the exclusion
is explicit, never silent, and a glob that names nothing says so — and every
path it names is foundation for every other kind, so a broader glob cannot
put it back.

`harnex lifecycle decisions [--tag T] [--decision D]` lists every
record in the decision ledger sorted by timestamp descending. Operators
audit the promote / reject / defer / demote history without reading
raw jsonl.

ConsumerDetector is a trait; built-in strategies are the `ConsumerStrategy`
variants (that enum is the source of truth — do not count them here):
- `grep` — reads every plain file the project owns (`harness-core::git`:
  tracked plus untracked not ignored by `.gitignore`; a submodule, a nested
  repository and a symlink's target are not read), matches the
  `{slug}`-substituted pattern, listed once at construction. Not a
  repository → `LIFECYCLE_GIT_FAILURE`, never a walk.
- `graph-backlinks` — calls `nodex query backlinks <node_id>` via the
  graph module. Fails explicitly if nodex is absent (never silent
  fallback to grep). Pattern field holds the node-id template.

A detector that cannot read its corpus — no repository, no nodex — fails
`classify`, which asked for that path; the sweep declares the kind in
`kinds_skipped` as `consumers unmeasured` and sweeps the rest.

When adding a new strategy: add a `ConsumerStrategy` variant (single
source of truth — `from_str`/`as_str`/`ALL` derive from it), add a
`ConsumerDetector` impl, add a match arm in `consumer_detector_for`
(exhaustive match enforces this step at compile time), add a test
asserting both happy and unknown-strategy paths.
