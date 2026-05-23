#!/usr/bin/env bash
set -uo pipefail

CHANGES=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
if [[ "$CHANGES" -gt 0 ]]; then
  echo "{\"additionalContext\": \"Warning: ${CHANGES} uncommitted files. Consider committing before ending the session.\"}"
fi

exit 0
