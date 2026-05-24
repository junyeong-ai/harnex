#!/usr/bin/env bash
# PostToolUse(Edit|Write): format the edited file with rustfmt. Advisory,
# always exits 0. file_path is parsed from stdin with jq — a compiled Rust
# repo has no bundled JSON interpreter, so jq is the canonical shell tool;
# if it is absent the hook skips rather than hand-roll fragile JSON parsing.
# Path traversal and non-existent files are skipped.
set -uo pipefail

command -v jq >/dev/null 2>&1 || exit 0

FILE=$(jq -r '.tool_input.file_path // ""' 2>/dev/null) || exit 0

[[ -z "$FILE" || "$FILE" == *..* || ! -f "$FILE" ]] && exit 0

case "$FILE" in
  *.rs) rustfmt "$FILE" >/dev/null 2>&1 || true ;;
esac
exit 0
