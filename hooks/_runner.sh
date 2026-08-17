#!/usr/bin/env bash
# Claude Code hook wrapper. Anchors cwd at the git root so a verifier's
# relative paths resolve the same wherever Claude fired the hook from,
# refuses path traversal in the verifier name, and dispatches by extension.
# Fails open on environment drift — a broken toolchain must never block an
# edit, and the gates remain the structural failure surface.
#
# One wrapper serves every language. What differs per ecosystem is which
# interpreter a verifier needs, so each non-shell arm probes its own and
# skips when it is absent. A `.sh` verifier that shells out to a formatter
# probes that formatter itself, which is why this file probes nothing for
# the shell arm: gating on a toolchain the verifier never calls would skip a
# working hook for an unrelated reason.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || { echo "[harnex-skipped: git root not found]" >&2; exit 0; }
cd "${ROOT}"

[[ $# -eq 0 ]] && { echo "[harnex-skipped: no script argument]" >&2; exit 0; }
SCRIPT="$1"; shift

case "$SCRIPT" in
  *..*) echo "[harnex-skipped: path traversal refused: $SCRIPT]" >&2; exit 0 ;;
esac

VERIFIER="${ROOT}/hooks/${SCRIPT}"

case "$SCRIPT" in
  *.sh)
    exec bash "${VERIFIER}" "$@"
    ;;
  *.py)
    # `--frozen` never mutates the lockfile: re-locking as a side effect of a
    # hook firing is a surprise, so drift skips and the developer re-syncs
    # deliberately.
    uv run --frozen python -c "" 2>/dev/null || { echo "[harnex-skipped: uv env unavailable — run 'uv sync']" >&2; exit 0; }
    exec uv run --frozen python "${VERIFIER}" "$@"
    ;;
  *.ts|*.js|*.mjs)
    command -v node >/dev/null 2>&1 || { echo "[harnex-skipped: node not found]" >&2; exit 0; }
    exec node "${VERIFIER}" "$@"
    ;;
  *)
    echo "[harnex-skipped: unsupported verifier extension: $SCRIPT]" >&2; exit 0
    ;;
esac
