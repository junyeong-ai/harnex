#!/usr/bin/env bash
# PostToolUse(Edit|Write): format the edited file with ruff. Formatter failures
# are advisory. file_path is parsed from stdin via python3 (present in a Python
# repo); _format-target.sh owns the project file boundary.
set -uo pipefail

INPUT=$(cat)
FILE=$(printf '%s' "$INPUT" | python3 -c "import json,sys;print((json.load(sys.stdin).get('tool_input') or {}).get('file_path',''))" 2>/dev/null) || exit 0

source "$(dirname -- "$0")/_format-target.sh" || exit 1
resolve_format_target "$FILE" || exit 0

case "$FILE" in
  *.py) ruff format "$FILE" >/dev/null 2>&1 || true ;;
esac
exit 0
