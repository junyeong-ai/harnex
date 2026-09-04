#!/usr/bin/env bash
# PostToolUse(Edit|Write): format the edited file with biome. Advisory,
# always exits 0. file_path is parsed from stdin via node (present in a TS
# repo); a path outside the project, a symlink, and a non-existent file are
# skipped.
set -uo pipefail

INPUT=$(cat)
FILE=$(node -e 'try{const j=JSON.parse(require("fs").readFileSync(0,"utf8"));process.stdout.write((j.tool_input||{}).file_path||"")}catch{}' <<<"$INPUT" 2>/dev/null) || exit 0

# The runtime hands an absolute path, so `..` never appears and a `..` test
# guards nothing on its own — the question is whether the file is under the
# project. A symlink is declined rather than resolved: formatting one writes
# through to its target, which the containment test below cannot see.
[[ -z "$FILE" || "$FILE" == *..* || -L "$FILE" || ! -f "$FILE" ]] && exit 0
case "$FILE" in
  /*) [[ "$FILE" == "$PWD"/* ]] || exit 0 ;;
esac

# Format only where this project declares biome. With no configuration biome
# applies its own defaults, and its search climbs to the home directory — so an
# unconfigured project has every edit rewritten against biome's defaults, or
# against whatever a parent directory happens to hold. Both fight the formatter
# the project actually runs. The dotted names are Biome 2 and later; on Biome 1
# they cannot exist, so checking them costs nothing.
[[ -f biome.json || -f biome.jsonc || -f .biome.json || -f .biome.jsonc ]] || exit 0

# Reach the biome the project can already run, never one fetched at edit time,
# and prefer the closest. Yarn Plug'n'Play generates no node_modules/.bin and
# gives `yarn <bin>` as the form to reach a workspace binary. A biome on PATH is
# a global install rather than the project's pin, but it is also what the
# project's own gate would run. A registry fetch inside a per-edit hook is a
# network round trip against the hook timeout, and an unpinned version besides.
if [[ -x node_modules/.bin/biome ]]; then
  BIOME=(node_modules/.bin/biome)
elif [[ -f .pnp.cjs ]] && command -v yarn >/dev/null 2>&1; then
  BIOME=(yarn biome)
elif command -v biome >/dev/null 2>&1; then
  BIOME=(biome)
else
  exit 0
fi

case "$FILE" in
  *.ts|*.tsx|*.js|*.jsx|*.json) "${BIOME[@]}" check --write "$FILE" >/dev/null 2>&1 || true ;;
esac
exit 0
