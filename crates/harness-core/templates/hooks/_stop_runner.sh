#!/usr/bin/env bash
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
cd "${ROOT}"

[[ $# -eq 0 ]] && exit 0
SCRIPT="$1"; shift

case "$SCRIPT" in
  *..*)  ;;
  *.sh)  bash "${ROOT}/hooks/$SCRIPT" "$@" 2>&1 || true ;;
esac

exit 0
