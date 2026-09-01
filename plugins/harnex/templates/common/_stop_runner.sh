#!/usr/bin/env bash
# Stop-class hook wrapper. ALWAYS exits 0 — a non-zero (exit 2) Stop hook
# forces Claude to keep going (re-stop loop). Anchors cwd at the project root
# and dispatches the named .sh verifier; the verifier's outcome is observed but
# never propagated. Rejects path traversal in the script-name argument.
#
# The anchors are the runtime's CLAUDE_PROJECT_DIR and this file's own
# location, never the working directory — see `_runner.sh` for why git cannot
# answer this. The hazard below is what that choice avoids: from inside a
# submodule, git names the inner repository, which is the other repository.
#
# The verifier's stdout IS the hook's control channel — a Stop verifier speaks
# JSON there — so its stderr stays stderr. Folding the two together lets any
# verifier that logs a diagnostic corrupt the JSON and lose the advisory.
set -uo pipefail

HOOKS="$(cd "$(dirname "$0")" && pwd)" || exit 0
ROOT="${CLAUDE_PROJECT_DIR:-${HOOKS%/*}}"
# Explicit, because there is no `set -e` here: an unguarded failure would fall
# through and the verifier would report on whatever directory the caller was
# in, naming another repository's uncommitted work as this session's.
cd "${ROOT}" 2>/dev/null || exit 0

[[ $# -eq 0 ]] && exit 0
SCRIPT="$1"; shift

VERIFIER="${HOOKS}/${SCRIPT}"

case "$SCRIPT" in
  *..*) ;;
  *.sh) [[ -f "${VERIFIER}" ]] && { bash "${VERIFIER}" "$@" || true; } ;;
esac

exit 0
