#!/usr/bin/env bash
# PreToolUse arm for `harnex guard floor` — the hook-bypass tripwire, and the
# enforcement-surface freeze where the project wires this for Edit|Write too.
#
# A shell arm rather than the binary wired directly, for two reasons a hook
# `command` cannot answer itself: it is exec form with no shell around it, so
# an absent oracle would fail this hook on every tool call; and a PreToolUse
# exit of 2 blocks the agent, which is also what an argument parser exits on,
# so an oracle too old to carry this subcommand would block every command it
# was asked about. One probe answers both — it exits 0 only where this exact
# subcommand parses, and anything else leaves the tool call alone.
#
# The binary speaks the PreToolUse contract itself, notice channel included.
# `exec` hands it this process, so its exit code and stdin are the arm's.
set -uo pipefail

harnex guard floor --help >/dev/null 2>&1 || exit 0

exec harnex guard floor
