#!/usr/bin/env bash
# Stop-class hook wrapper. ALWAYS exits 0 — a non-zero (exit 2) Stop hook
# forces Claude to keep going (re-stop loop). Anchors cwd at the git root and
# dispatches the named .sh verifier; the verifier's outcome is observed but
# never propagated. Rejects path traversal in the script-name argument.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
cd "${ROOT}"

[[ $# -eq 0 ]] && exit 0
SCRIPT="$1"; shift

case "$SCRIPT" in
  *..*) ;;
  *.sh) bash "${ROOT}/hooks/$SCRIPT" "$@" 2>&1 || true ;;
esac

exit 0
