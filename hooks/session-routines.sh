#!/usr/bin/env bash
# SessionStart: surface routines that are overdue or never scheduled, so a
# session opens knowing what the harness is owed. Absence of the oracle is
# install-to-enable silence; everything past that failure-reports in one
# line, because a blank surface over a broken schedule is indistinguishable
# from a clean one. Always exit 0 — this hook never blocks a session.
set -uo pipefail
command -v harnex >/dev/null 2>&1 || exit 0
if ! command -v jq >/dev/null 2>&1; then
  echo "Routines: jq not installed — schedule not surfaced"
  exit 0
fi
envelope=$(harnex lifecycle routines 2>/dev/null) || true
if [ "$(printf '%s' "$envelope" | jq -r '.ok' 2>/dev/null)" != "true" ]; then
  echo "Routines: schedule unreadable — run harnex check"
  exit 0
fi
printf '%s' "$envelope" | jq -r '
  .data.items[]
  | select(.state == "overdue" or .state == "unscheduled")
  | "Routine \(.state): \(.slug) (\(.cadence), owner \(.owner)) — produces \(.produces // "unset")"
' 2>/dev/null || echo "Routines: schedule unreadable — run harnex check"
exit 0
