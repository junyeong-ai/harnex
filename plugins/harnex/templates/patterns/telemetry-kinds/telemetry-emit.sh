#!/usr/bin/env bash
# PostToolUse / PostToolUseFailure: hand the hook event to the oracle, which
# records one harness_invocation event — the invoked element's slug and whether
# the call succeeded. Everything the seam decides (the tool → element mapping,
# outcome from the event, what may cross) lives in `harnex guard telemetry-emit`
# so nothing is duplicated in shell. Wire it through `_runner.sh` (which execs
# it via bash, so the template ships without an executable bit), to BOTH events
# with matcher `Skill|Task|Agent`, best `async` so the append never sits on the
# tool's critical path.
#
# Install-to-enable and unconditionally silent: a no-op when harnex is absent,
# and any delegated failure — an older harnex without the subcommand included —
# is suppressed, because telemetry must never surface an error onto the tool
# call it observed. The oracle owns every recording decision; this wrapper owns
# only that the tool call is never disturbed.
set -uo pipefail
command -v harnex >/dev/null 2>&1 || exit 0
harnex guard telemetry-emit >/dev/null 2>&1 || true
exit 0
