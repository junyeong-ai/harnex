---
paths:
  - "crates/harness-core/src/session/**"
  - "crates/harness-cli/src/commands/session.rs"
---

# session — reading Claude Code's own transcripts

## Who owns what

| module | owns |
|---|---|
| `record` | the transcript vocabulary, and every constant measured out of it |
| `submission` | the instruction boundary, decided once for every analyser |
| `prompt` | exact-paragraph repetition, across sessions and inside one — a
  paragraph is usually both and is counted in both, since the two failures
  want opposite fixes |
| `intervention` | steering and marked interrupts |
| `harness` | what the project's harness did, and what it cost |
| `rework` | edits to a file after the commit that shipped it |
| `repository` | what became of a commit, through git, project scope only |
| `baseline` | frozen rates, the refusal to compare overlapping windows,
  which rates a comparison will withhold, and the trend that lays one
  scope's windows side by side without subtracting |
| `discovery` | the roots, absolute and deduplicated |

Each module doc argues its own refusals. Read the doc before changing the
behaviour it argues for; do not restate it here.

## Every constant carries its measurement

An upstream string — a prompt source, a record subtype, a tool name, an input
key — is a guess until it is counted against the corpus. A new one arrives with
the number that justified it in its doc comment, in the shape the existing ones
use. A signal the runtime writes on only part of a population is published as a
floor, named for that, and carries the coverage it was measured at.

## Invariants that a change can break silently

- **A transcript's own order is authoritative; its timestamps are not.** A
  session's files are interleaved by `interleave_by_time`
  ([file: crates/harness-core/src/session/mod.rs]), which never reorders within
  one — 2.27% of adjacent records are stamped behind the record before them.
  Sorting the concatenation is the shape that looks equivalent and is not.
- **One rule decides whether a rate can be compared.** `Measurement::supports`
  ([file: crates/harness-core/src/session/baseline.rs]) is what `diff` withholds
  on and what `baseline save` discloses; a second copy of the floor check in
  either place drifts silently, because both still return plausible answers.
- **Coverage counts the window, not the file.** `read_transcript` applies
  `since` and `project` before counting, because `require_coverage` gates on
  the ratio coverage publishes.
- **One word, one meaning across the envelope.** `authored` in
  `Coverage::user_turns_by_authorship` and `PromptFacts::authored_turns` are
  the same population; a test asserts they agree rather than asserting either
  number.
- **Verbatim operator content is opt-in.** Prompt text, and the input of a
  refused call, appear only under `with_text`; grouping happens on them either
  way. A harness element's own name is not operator content and is reported
  plainly.
- **`serde_json` is `preserve_order` here**, pulled in by `schemars`, so a
  `Value` serialises in insertion order. Canonicalise before using one as a
  map key — `canonical` ([file: crates/harness-core/src/session/harness.rs]) does,
  and its test asserts the ordering it exists for.
- **A commit is a floor, not a count.** The runtime attaches `gitOperation` to
  some commits and not others — 29 of git's 42 over this project. Anything
  denominated in commits reads high, and `repository.authored_in_span` reports
  what the floor is a floor against so a consumer can see the gap.
- **Never pipe stdin to a subprocess that also writes stdout.** The repository
  survey ([file: crates/harness-core/src/session/repository.rs]) passes its
  query as a file; a piped write deadlocks once the child's output fills the
  kernel buffer, measured between six and eight thousand commits.

## Adding to a closed set

`SessionMetric`, `InterventionKind` and `CommitFate` are wire vocabularies: a
baseline written by one build is read by another, and a report is written
against the names. Add one line to the `wire_enum!` block (it writes `ALL`, `from_str` and
`as_str`, and the exhaustive match forces the rest). Where a shipped document
depends on the new name, add it to the `CONTRACTS` table
([file: crates/harness-core/tests/plugin_prose_sync.rs]) as `Type.field` **and
move that document's declared citation count**: the count is the guard's
denominator, so a document that gains a citation fails the build until the
citation is either watched or the count is moved deliberately.

A variant no input can produce is deleted rather than kept — `CommitFate` had
one until git was asked what it actually answers.

## What the oracle does not do

Judging belongs to the judge ([file: plugins/harnex/agents/session-judge.md]).
Nothing here scores a prompt, names a cause, or identifies a turn by its
wording. A metric whose name implies a verdict is the defect, not the number
under it.
