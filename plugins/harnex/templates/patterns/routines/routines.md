---
paths:
  - ".claude/routines/**"
governs:
  concept: the scheduled harness tasks and their records
  live_truth:
    - .claude/routines
---

# Routines — recurring harness work with a cadence and a record

A routine is a file under `.claude/routines/`: frontmatter schedules it,
`prompt:` is the work addressed to whichever session picks it up, and the
body is the running record. `harnex lifecycle routines` answers where each
stands today; the SessionStart hook surfaces what is overdue or never
scheduled, so a session opens knowing what the harness is owed.

## The contract

- **`produces:` presence is completion.** The record file is the proof of
  work — a routine is done when the artifact it promised exists, never when
  someone says so.
- **The next tick is scheduled by hand.** After producing, set the next
  `when:` and the next `produces:` yourself. A date derived from when the
  last run happened to land drifts the cadence toward slippage.
- **Overdue never gates.** Schedule state surfaces at session start and in
  the query; it is deliberately not a `check` finding, because a gate whose
  verdict moves with the clock alone fails a tree nothing changed.
- **Retire in place.** `status: superseded` keeps the file as its own
  record; deleting it deletes the history of why the cadence existed.
- **`unscheduled` is loud on purpose.** A routine missing `when:` or
  `produces:` was installed and never given its first tick — schedule it or
  supersede it, and never leave it silently idle.
