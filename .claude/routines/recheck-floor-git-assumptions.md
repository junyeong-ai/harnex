---
when: 2027-03-26
cadence: semiannual
owner: harness
produces: .harness/records/2027-h1-floor-git.md
prompt: |
  Re-measure the git behaviours `guard::floor::bypass` is built on, against
  the git installed today, and write the result to the path in produces:.

  The module's tests assert harnex's verdict on a command line. They cannot
  assert the half the verdict rests on — that git still reads that line the
  way the module assumes — so the suite stays green while the assumption
  rots. Nothing else in this repository looks at that class: `spec` stamps
  Claude Code's surface, and no gate reads git's.

  Measure, do not reason. For each claim below, run it against a throwaway
  repository with a `pre-commit` that exits 1, and record whether the hook
  ran and whether `git config --get core.hooksPath` answers:

  - `--no-verify` resolves from any unambiguous prefix, and `--no-ver` is
    ambiguous against `--no-verbose`.
  - `commit -n` is `--no-verify` while `push -n` and `merge -n` are dry-run.
  - `git config` reaches core.hooksPath through every write form the module
    enumerates, including the valueless ones, and through no read form.
  - A global option that takes a separate value is one of the enumerated
    set; a new one turns its value into a false subcommand and hides the
    flag behind it.
  - `--config-env` applies the key attached and separated, and git rejects
    every abbreviation of it.
  - `GIT_CONFIG_PARAMETERS` applies a pair in either quoting and at any
    position, and refuses an unquoted list outright.
  - `GIT_CONFIG_KEY_<n>` applies the key whatever its case.

  A claim that moved is a floor that no longer holds: fix the module in the
  same pass. Name the git version in the file produces: points at, and add a
  dated line to the record below, because that is where the next tick reads
  what the last one measured against.
---

# Record

## 2026-09-26 — baseline, git 2.55.0

Every claim above measured true. Established while closing the gap where
core.hooksPath carried in through the environment reached git's configuration
and the floor passed it: `GIT_CONFIG_PARAMETERS`, the `GIT_CONFIG_KEY_<n>`
triple, and `--config-env` in both spellings. The enumerated set of
value-taking global options was short by two, which turned their value into a
false subcommand and hid the flag behind it.

Abbreviations of `--config-env` were rejected by git (exit 129), and an
unquoted `GIT_CONFIG_PARAMETERS` was refused as `bogus format`, so neither is
a spelling the floor has to carry.
