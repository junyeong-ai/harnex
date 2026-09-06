#!/usr/bin/env bash
# Stop verifier: surface uncommitted work as a non-blocking advisory. Never
# blocks — only `decision` and exit 2 do that. The Stop event delivers two
# channels and they differ by reader: `systemMessage` becomes a
# `hook_system_message` record and `hookSpecificOutput.additionalContext` a
# `hook_additional_context` one, which the stop summary also keeps a field for
# as the model's feedback channel. This is a nudge to the person, so it takes
# theirs. Pure bash — the only interpolated value is an integer count, so no
# JSON escaping (and no language runtime) is needed.
set -uo pipefail

CHANGES=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
if [[ "${CHANGES:-0}" -gt 0 ]]; then
  echo "{\"systemMessage\": \"${CHANGES} uncommitted file(s). Consider committing before ending the session.\"}"
fi
exit 0
