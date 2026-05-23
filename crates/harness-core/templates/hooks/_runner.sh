#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "[harness-skipped: git root not found]" >&2
  exit 0
}
cd "${ROOT}"

[[ $# -eq 0 ]] && { echo "[harness-skipped: no script argument]" >&2; exit 0; }
SCRIPT="$1"; shift

case "$SCRIPT" in
  *..*)  echo "[harness-skipped: path traversal refused: $SCRIPT]" >&2; exit 0 ;;
  *.sh)  exec bash "${ROOT}/hooks/$SCRIPT" "$@" ;;
  *)     echo "[harness-skipped: unknown extension: $SCRIPT]" >&2; exit 0 ;;
esac
