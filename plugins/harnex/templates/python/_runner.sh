#!/usr/bin/env bash
# Hook wrapper (Python / uv). Anchor cwd at git root, probe the uv environment,
# fail open on drift, dispatch the named verifier. Rejects path traversal.
# Does NOT auto-`uv lock`: mutating the lockfile as a side effect of a hook
# firing is surprising; on drift it skips and the developer re-syncs explicitly.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || { echo "[harnex-skipped: git root not found]" >&2; exit 0; }
cd "${ROOT}"

[[ $# -eq 0 ]] && { echo "[harnex-skipped: no script argument]" >&2; exit 0; }
SCRIPT="$1"; shift

uv run --frozen python -c "" 2>/dev/null || { echo "[harnex-skipped: uv env unavailable — run 'uv sync']" >&2; exit 0; }

case "$SCRIPT" in
  *..*) echo "[harnex-skipped: path traversal refused: $SCRIPT]" >&2; exit 0 ;;
  *.py) exec uv run --frozen python "${ROOT}/hooks/$SCRIPT" "$@" ;;
  *.sh) exec bash "${ROOT}/hooks/$SCRIPT" "$@" ;;
  *)    echo "[harnex-skipped: unknown extension: $SCRIPT]" >&2; exit 0 ;;
esac
