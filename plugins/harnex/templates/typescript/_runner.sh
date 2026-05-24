#!/usr/bin/env bash
# Hook wrapper (TypeScript / Node). Anchor cwd at git root, probe node,
# dispatch the named verifier. Fail-open on env drift — never penalize the
# developer for a broken toolchain. Rejects path traversal.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || { echo "[harnex-skipped: git root not found]" >&2; exit 0; }
cd "${ROOT}"

[[ $# -eq 0 ]] && { echo "[harnex-skipped: no script argument]" >&2; exit 0; }
SCRIPT="$1"; shift

command -v node >/dev/null 2>&1 || { echo "[harnex-skipped: node not found]" >&2; exit 0; }

case "$SCRIPT" in
  *..*) echo "[harnex-skipped: path traversal refused: $SCRIPT]" >&2; exit 0 ;;
  *.ts) exec node "${ROOT}/hooks/$SCRIPT" "$@" ;;
  *.sh) exec bash "${ROOT}/hooks/$SCRIPT" "$@" ;;
  *)    echo "[harnex-skipped: unknown extension: $SCRIPT]" >&2; exit 0 ;;
esac
