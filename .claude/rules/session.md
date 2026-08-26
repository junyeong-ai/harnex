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
| `prompt` | exact-paragraph repetition, across sessions and inside one |
| `intervention` | steering and marked interrupts |
| `harness` | what the project's harness did, and what it cost |
| `rework` | edits to a file after the commit that shipped it |
| `repository` | what became of a commit, through git, project scope only |
| `baseline` | frozen rates, and the refusal to compare overlapping windows |
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
- **Never pipe stdin to a subprocess that also writes stdout.** The repository
  survey ([file: crates/harness-core/src/session/repository.rs]) passes its
  query as a file; a piped write deadlocks once the child's output fills the
  kernel buffer, measured between six and eight thousand commits.

## Adding to a closed set

`SessionMetric`, `InterventionKind` and `CommitFate` are wire vocabularies: a
baseline written by one build is read by another, and a report is written
against the names. Add a variant to the enum, its `ALL`, `from_str` and
`as_str` (the exhaustive match forces the rest), and add the name to the
`CONTRACTS` table ([file: crates/harness-core/tests/plugin_prose_sync.rs]) when a
shipped document depends on it.

A variant no input can produce is deleted rather than kept — `CommitFate` had
one until git was asked what it actually answers.

## What the oracle does not do

Judging belongs to the judge ([file: plugins/harnex/agents/session-judge.md]).
Nothing here scores a prompt, names a cause, or identifies a turn by its
wording. A metric whose name implies a verdict is the defect, not the number
under it.
