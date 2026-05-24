#!/usr/bin/env bash
# Hook wrapper (Python / uv). Anchor cwd at git root, probe the uv environment,
# self-heal once on drift (uv lock), else fail-open. Dispatch the named
# verifier. Rejects path traversal.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || { echo "[harnex-skipped: git root not found]" >&2; exit 0; }
cd "${ROOT}"

[[ $# -eq 0 ]] && { echo "[harnex-skipped: no script argument]" >&2; exit 0; }
SCRIPT="$1"; shift

if ! uv run --frozen python -c "" 2>/dev/null; then
  uv lock >/dev/null 2>&1 || { echo "[harnex-skipped: uv env unavailable]" >&2; exit 0; }
fi

case "$SCRIPT" in
  *..*) echo "[harnex-skipped: path traversal refused: $SCRIPT]" >&2; exit 0 ;;
  *.py) exec uv run --frozen python "${ROOT}/hooks/$SCRIPT" "$@" ;;
  *.sh) exec bash "${ROOT}/hooks/$SCRIPT" "$@" ;;
  *)    echo "[harnex-skipped: unknown extension: $SCRIPT]" >&2; exit 0 ;;
esac
