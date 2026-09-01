#!/usr/bin/env bash
# PostToolUse / PostToolUseFailure: record one harness-invocation event —
# the surface identifier and whether the call succeeded. Wire it to BOTH
# events with matcher `Skill|mcp__.*`; the outcome is which event fired, so
# a failure can never be recorded as a success. Only the identifier crosses:
# never the tool_input, never content, never anything a person typed — the
# Kind's closed payload_schema rejects the rest anyway.
#
# Install-to-enable: a no-op when harnex or jq is absent. Always exit 0 —
# telemetry never blocks a tool call. Best wired `async` so the append never
# sits on the tool's critical path.
set -uo pipefail
command -v harnex >/dev/null 2>&1 || exit 0
command -v jq >/dev/null 2>&1 || exit 0

input=$(cat)

# The outcome is the event's identity, not a field in the payload.
event=$(printf '%s' "$input" | jq -r '.hook_event_name // ""' 2>/dev/null)
case "$event" in
  PostToolUse) outcome=ok ;;
  PostToolUseFailure) outcome=failed ;;
  *) exit 0 ;;  # not an outcome-bearing event
esac

# The surface identifier: a Skill call names the skill in tool_input.name;
# an MCP tool is its own tool_name. Nothing else is read.
surface=$(printf '%s' "$input" | jq -r '
  if .tool_name == "Skill" then (.tool_input.name // "Skill")
  else (.tool_name // "")
  end
' 2>/dev/null)
[ -n "$surface" ] || exit 0

harnex telemetry append --kind harness_invocation \
  --payload "$(jq -cn --arg s "$surface" --arg o "$outcome" \
    '{surface:$s, outcome:$o}')" >/dev/null 2>&1 || true
exit 0
