#!/usr/bin/env bash
# PreToolUse arm for `harnex guard floor` — the hook-bypass tripwire, and the
# enforcement-surface freeze where the project wires this for Edit|Write too.
#
# A shell arm rather than the binary wired directly, for two reasons a hook
# `command` cannot answer itself: it is exec form with no shell around it, so
# an absent oracle would fail this hook on every tool call; and a PreToolUse
# exit of 2 blocks the agent, which is also what an argument parser exits on,
# so an oracle too old to carry this subcommand would block every command it
# was asked about. The probes below leave the tool call alone in both cases,
# and separate out the third: an oracle on PATH that does not run at all.
#
# The binary speaks the PreToolUse contract itself, notice channel included.
# `exec` hands it this process, so its exit code and stdin are the arm's.
set -uo pipefail

# A machine that never installed the oracle is not told so on every tool call.
command -v harnex >/dev/null 2>&1 || exit 0

# `--help` and not `--version`: clap always carries the first and the second is
# declared, so the narrower flag would read a running oracle as broken. An
# oracle answering neither is on PATH and does not run — a version manager's
# shim with nothing selected, a dangling symlink, an interrupted install — and
# there the floor is down while `command -v` still names a binary, so silence
# would read as a floor standing.
#
# The notice takes the channel the binary's own skips take. A hook that exits 0
# is read for the control JSON on stdout and not for stderr, so a message
# written to stderr here would reach no one.
harnex guard floor --help >/dev/null 2>&1 || {
  if ! harnex --help >/dev/null 2>&1; then
    where=$(command -v harnex)
    where=${where//\\/\\\\}
    where=${where//\"/\\\"}
    where=${where//$'\n'/ }
    printf '{"systemMessage":"[floor-check skipped: %s does not run]","suppressOutput":true}\n' "$where"
  fi
  exit 0
}

exec harnex guard floor
