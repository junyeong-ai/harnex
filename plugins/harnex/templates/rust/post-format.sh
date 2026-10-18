#!/usr/bin/env bash
# PostToolUse(Edit|Write): format the edited file with rustfmt. Formatter failures
# are advisory. file_path is parsed from stdin with jq — a compiled Rust
# repo has no bundled JSON interpreter, so jq is the canonical shell tool;
# if it is absent the hook skips rather than hand-roll fragile JSON parsing.
# _format-target.sh owns the project file boundary.
#
# Invoked per file, rustfmt reads its edition from `rustfmt.toml` — it does
# NOT see Cargo.toml, and its own default is edition 2015. Without that file
# the hook formats to a different style than `cargo fmt` and every edit
# reverts what the gate requires, so the scaffold emits one alongside this
# hook carrying the project's declared edition.
set -uo pipefail

command -v jq >/dev/null 2>&1 || exit 0

FILE=$(jq -r '.tool_input.file_path // ""' 2>/dev/null) || exit 0

source "$(dirname -- "$0")/_format-target.sh" || exit 1
resolve_format_target "$FILE" || exit 0

case "$FILE" in
  *.rs) rustfmt "$FILE" >/dev/null 2>&1 || true ;;
esac
exit 0
