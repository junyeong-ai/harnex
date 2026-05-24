#!/usr/bin/env bash
# PostToolUse(Edit|Write): format the edited file with ruff. Advisory, always
# exits 0. file_path is parsed from stdin via python3 (present in a Python
# repo); path traversal and non-existent files are skipped.
set -uo pipefail

INPUT=$(cat)
FILE=$(printf '%s' "$INPUT" | python3 -c "import json,sys;print((json.load(sys.stdin).get('tool_input') or {}).get('file_path',''))" 2>/dev/null) || exit 0

[[ -z "$FILE" || "$FILE" == *..* || ! -f "$FILE" ]] && exit 0

case "$FILE" in
  *.py)
    ruff format "$FILE" >/dev/null 2>&1 || true
    ruff check --fix "$FILE" >/dev/null 2>&1 || true
    ;;
esac
exit 0
