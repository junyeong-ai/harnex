#!/usr/bin/env bash
# Stop verifier: surface uncommitted work as advisory context. Never blocks.
# Pure bash — the only interpolated value is an integer count, so no JSON
# escaping (and no language runtime) is needed.
set -uo pipefail

CHANGES=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
if [[ "${CHANGES:-0}" -gt 0 ]]; then
  echo "{\"additionalContext\": \"${CHANGES} uncommitted file(s). Consider committing before ending the session.\"}"
fi
exit 0
