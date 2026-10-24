#!/usr/bin/env bash
# PostToolUse(Edit|Write): format the edited file with biome. Formatter
# failures are advisory. file_path is parsed from stdin via node (present in a TS
# repo); _format-target.sh owns the project file boundary.
set -uo pipefail

INPUT=$(cat)
FILE=$(node -e 'try{const j=JSON.parse(require("fs").readFileSync(0,"utf8"));process.stdout.write((j.tool_input||{}).file_path||"")}catch{}' <<<"$INPUT" 2>/dev/null) || exit 0

source "$(dirname -- "$0")/_format-target.sh" || exit 1
resolve_format_target "$FILE" || exit 0

# Stay inside the project when discovering configuration and installed tools.
ROOT=$FORMAT_ROOT
SEARCH=${FILE%/*}
SEARCH=${SEARCH:-/}
CONFIG=
BIOME=()
PNP_ROOT=
while :; do
  if [[ -z "$CONFIG" ]] &&
    [[ -f "$SEARCH/biome.json" || -f "$SEARCH/biome.jsonc" ||
       -f "$SEARCH/.biome.json" || -f "$SEARCH/.biome.jsonc" ]]; then
    CONFIG=$SEARCH
  fi
  if [[ ${#BIOME[@]} -eq 0 ]]; then
    if [[ -x "$SEARCH/node_modules/.bin/biome" ]]; then
      BIOME=("$SEARCH/node_modules/.bin/biome")
    elif [[ -f "$SEARCH/.pnp.cjs" ]] && command -v yarn >/dev/null 2>&1; then
      BIOME=(yarn)
      PNP_ROOT=$SEARCH
    fi
  fi
  [[ "$SEARCH" == "$ROOT" ]] && break
  SEARCH=${SEARCH%/*}
  SEARCH=${SEARCH:-/}
done
[[ -n "$CONFIG" ]] || exit 0
if [[ -n "$PNP_ROOT" ]]; then
  BIOME=(yarn --cwd "$PNP_ROOT" exec yarn --cwd "$CONFIG" exec biome)
fi

# A PATH installation is used only after the project declared Biome and no
# project-local installation was found. No runner here fetches a package.
if [[ ${#BIOME[@]} -eq 0 ]] && command -v biome >/dev/null 2>&1; then
  BIOME=(biome)
fi
[[ ${#BIOME[@]} -gt 0 ]] || exit 0
cd -- "$CONFIG" || exit 0

case "$FILE" in
  *.ts|*.tsx|*.js|*.jsx|*.json) "${BIOME[@]}" check --write --linter-enabled=false "$FILE" >/dev/null 2>&1 || true ;;
esac
exit 0
