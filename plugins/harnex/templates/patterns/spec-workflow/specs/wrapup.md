# <title> — Wrapup

Written at the wrapup phase, once `review` and `acceptance` have both recorded
`approved`. Set the `spec.md` frontmatter `status` to `completed` in the same
commit.

`completed` means the spec's promise was kept and observed, so an `acceptance`
that ended `deferred` does not reach here: the spec stays in flight until the
gate re-fires `approved`, or it is abandoned or superseded with the record
saying which criteria went unanswered.

## Result

<What now holds that did not before. The `acceptance` gate already walked the
criteria and its verdict is in the decision log; this records the durable half
for a reader who will not open the log — what each criterion got.>

## What the work revealed

<Observations from doing it — where the plan was wrong, what took longer than
expected, what the codebase turned out to be. This is the raw material the
promotion gate reads; write what happened, not what should have.>

## Left open

<Every row of `plan.md ## Outstanding issues` that still has no terminal
disposition, and every pre-existing defect `## Risks` records, one line each:
carried, naming where it went — an issue, a later spec — or dropped, with why. Retiring deletes `plan.md`, so a row this section
does not name leaves with the directory unread. Carry what someone will act
on; a list kept for its own sake grows at the rate reviews file findings.>

## Harness proposals

<Candidate rules, hooks, or skills this spec argues for, and what each would
enforce. Write what this spec saw; whether it has recurred enough to promote
is the ledger's answer, not this file's.>

Record one observation per proposal before this directory is retired, in the
standing wording where the tag already holds one:

```
harnex lifecycle observations --tag <topic>
harnex lifecycle observe --tag <topic> --text "<what recurs>" --source <slug>
```

Completing the spec removes this directory, so a proposal left only here is
one the promotion pass never sees. `.claude/rules/governance.md` has why the
wording is reused verbatim, and owns the bar each proposal is judged against.
Do not write the rule from here.

If those commands cannot run, why decides the next step:

- **No `[lifecycle]` in `harness.toml`** — the project keeps no ledger, and
  the commit body is the home governance names for an observation. Write each
  proposal into the body of the commit that retires this directory, and retire.
- **`[lifecycle]` declared, and no `harnex` on this machine or one the pin
  does not admit** — the ledger exists and only this machine cannot reach it.
  Install the release `[meta] harnex_version` admits — the session-start line
  or the refusal's hint names the command — and record; the directory stands
  only until then. A proposal written where
  the project's survey never reads is lost as surely as a deleted one.
