#!/usr/bin/env bash
# Stop verifier: surface uncommitted work as a non-blocking advisory. Never
# blocks — `hookSpecificOutput.additionalContext` is delivered without
# preventing the stop, which `decision` and exit 2 are the only things that do.
# A `systemMessage` written here would reach the transcript's raw stdout and no
# reader. Pure bash — the only interpolated value is an integer count, so no
# JSON escaping (and no language runtime) is needed.
set -uo pipefail

CHANGES=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
if [[ "${CHANGES:-0}" -gt 0 ]]; then
  printf '{"hookSpecificOutput":{"hookEventName":"Stop","additionalContext":"%s file(s) in the work tree are uncommitted."}}\n' \
    "$CHANGES"
fi
exit 0
