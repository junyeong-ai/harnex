#!/usr/bin/env bash
# PostToolUse / PostToolUseFailure: hand the hook event to the oracle, which
# records one harness_invocation event — the invoked element's slug and whether
# the call succeeded. Everything the seam decides (the tool → element mapping,
# outcome from the event, what may cross) lives in `harnex guard telemetry-emit`
# so nothing is duplicated in shell. Wire to BOTH events with matcher
# `Skill|Task|Agent`, best `async` so the append never sits on the tool's
# critical path.
#
# Install-to-enable: a no-op when harnex is absent. The oracle owns always-exit-0
# and every silent skip; this wrapper adds only the absent-oracle case.
set -uo pipefail
command -v harnex >/dev/null 2>&1 || exit 0
exec harnex guard telemetry-emit
