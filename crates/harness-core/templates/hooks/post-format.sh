#!/usr/bin/env bash
set -uo pipefail

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | python3 -c "import json,sys; print(json.load(sys.stdin).get('tool_input',{}).get('file_path',''))" 2>/dev/null) || exit 0

[[ -z "$FILE_PATH" ]] && exit 0
[[ "$FILE_PATH" == *".."* ]] && exit 0
[[ -f "$FILE_PATH" ]] || exit 0

case "$FILE_PATH" in
  *.rs)          cargo fmt -- "$FILE_PATH" 2>/dev/null || true ;;
  *.py)          ruff format "$FILE_PATH" 2>/dev/null || true ;;
  *.ts|*.tsx)    npx biome format --write "$FILE_PATH" 2>/dev/null || true ;;
  *.json)        npx biome format --write "$FILE_PATH" 2>/dev/null || true ;;
esac

exit 0
