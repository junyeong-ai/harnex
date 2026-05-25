#!/usr/bin/env bash
# Hook wrapper (Rust / cargo). Anchor cwd at git root, probe the cargo
# toolchain, dispatch the named verifier. Fail-open on env drift — never
# penalize the developer for a broken toolchain. Rejects path traversal.
# Rust verifiers are shell scripts that drive cargo (fmt/clippy/test); there
# is no per-hook `.rs` compilation, which would be slow and un-idiomatic.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || { echo "[harnex-skipped: git root not found]" >&2; exit 0; }
cd "${ROOT}"

[[ $# -eq 0 ]] && { echo "[harnex-skipped: no script argument]" >&2; exit 0; }
SCRIPT="$1"; shift

command -v cargo >/dev/null 2>&1 || { echo "[harnex-skipped: cargo not found]" >&2; exit 0; }

case "$SCRIPT" in
  *..*) echo "[harnex-skipped: path traversal refused: $SCRIPT]" >&2; exit 0 ;;
  *.sh) exec bash "${ROOT}/hooks/$SCRIPT" "$@" ;;
  *)    echo "[harnex-skipped: unsupported extension (Rust hooks are .sh): $SCRIPT]" >&2; exit 0 ;;
esac
