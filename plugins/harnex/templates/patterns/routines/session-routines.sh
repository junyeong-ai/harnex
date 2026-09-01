#!/usr/bin/env bash
# SessionStart: surface routines that are overdue or never scheduled, so a
# session opens knowing what the harness is owed. Install-to-enable, twice:
# without the oracle or without jq this prints nothing and exits 0 —
# absence is a no-op, never a gate failure.
set -uo pipefail
command -v harnex >/dev/null 2>&1 || exit 0
command -v jq >/dev/null 2>&1 || exit 0
harnex lifecycle routines 2>/dev/null | jq -r '
  .data.items[]
  | select(.state == "overdue" or .state == "unscheduled")
  | "Routine \(.state): \(.slug) (\(.cadence), owner \(.owner)) — produces \(.produces // "unset")"
' 2>/dev/null || :
exit 0
