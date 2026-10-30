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

Retirement classifier emits three signals: Stale (last change older than
stale_days), NoConsumers (grep finds zero), Silent (caller-supplied
`SilenceState` — derived from the telemetry query). Severity: 3 signals →
Major, 2 → Minor, ≤1 → Info. `SilenceState` is tri-state — `Silent` fires the signal;
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
never whether it is used. The load question has an owner: `retire.md` answers it
from `facts.harness.rule_loads`, and states the window that evidence needs.

Age, which Stale and the grace period read, is the caller's `LastChange` from
`last_changes`, read from the whole repository holding the project: the last
commit that changed the file on this branch, a merge dated by what it changed
against the branch it landed on, or now while the working tree holds a change no
commit holds yet, an edit or a file neither committed nor ignored. A directory
artifact, a skill, is dated by the files under it, since git lists only files.
What no history here records — a file outside the repository, inside one of its
own, or ignored — has `age_days` null, so it is neither Stale nor in its grace
period. An artifact's path is resolved as the system resolves it, and every link
the resolution passes through, its own, a directory's or one in a chain, is part
of it, so repointing one changes it. One outside the repository is recorded by
no history and leaves the artifact's `age_days` null, unless it leads to the
project or above it, `/var` on macOS, and so locates the project rather than the
artifact. Where the path resolves is dated as git records it, a link among a
directory artifact's files as that link. A name matches as `git status` matches
it, composed under `core.precomposeUnicode` on macOS, the one system git reads
it on, and case-folded under `core.ignorecase`; the rest of the user's git
configuration may not reshape the log the age is parsed from. Never a
modification time — a checkout writes every file's, so a fresh clone or a CI
runner reads every artifact as new and inside its grace period. A shallow clone
fails with `LIFECYCLE_GIT_FAILURE`, because its boundary commit dates every file
it holds to the day the clone was cut. One history walk and one instant, the
sweep's now, answer the whole sweep.

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
fabricated `Silent`. A match the sweep cannot read, a link whose target is
gone among them, is declared in `kinds_skipped` and the rest is swept; the
explicit ask, `classify`, fails on it. `harnex lifecycle retire` covers the
entire surface in one call.

When a kind is `foundation: true`, the sweep adds it to `kinds_skipped`
with a reason carrying how many paths its glob carved out — the exclusion
is explicit, never silent, and a glob that names nothing says so — and every
path it names is foundation for every other kind, so a broader glob cannot
put it back.

`harnex lifecycle decisions [--tag T] [--decision D]` lists every
record in the decision ledger sorted by timestamp descending. Operators
audit the promote / reject / defer / demote history without reading
raw jsonl.

`ConsumerDetector` reads every plain file the project owns
(`harness-core::git`: tracked plus untracked not ignored by `.gitignore`; a
submodule, a nested repository and a symlink's target are not read), listed
once at construction, and matches the `{slug}`-substituted pattern. Not a
repository → `LIFECYCLE_GIT_FAILURE`, never a walk. Keep the corpus every file
the project owns: a consumer outside it reads as `NoConsumers`, which
recommends retiring an artifact something still uses.

A detector that cannot read its corpus fails `classify`, which asked for that
path; the sweep declares the kind in `kinds_skipped` as `consumers unmeasured`
and sweeps the rest.
