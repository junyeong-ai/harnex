#!/usr/bin/env bash
# PostToolUse(Edit|Write): format the edited file with biome. Advisory,
# always exits 0. file_path is parsed from stdin via node (present in a TS
# repo); path traversal and non-existent files are skipped.
set -uo pipefail

INPUT=$(cat)
FILE=$(node -e 'try{const j=JSON.parse(require("fs").readFileSync(0,"utf8"));process.stdout.write((j.tool_input||{}).file_path||"")}catch{}' <<<"$INPUT" 2>/dev/null) || exit 0

[[ -z "$FILE" || "$FILE" == *..* || ! -f "$FILE" ]] && exit 0

# Format only where this project declares biome. With no configuration biome
# applies its own defaults, and its search climbs to the home directory — so an
# unconfigured project has every edit rewritten against biome's defaults, or
# against whatever a parent directory happens to hold. Both fight the formatter
# the project actually runs.
[[ -f biome.json || -f biome.jsonc || -f .biome.json || -f .biome.jsonc ]] || exit 0

case "$FILE" in
  *.ts|*.tsx|*.js|*.jsx|*.json) npx biome check --write "$FILE" >/dev/null 2>&1 || true ;;
esac
exit 0
