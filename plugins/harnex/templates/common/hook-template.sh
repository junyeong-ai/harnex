#!/usr/bin/env bash
# The verifier body `extend hook` starts from: a PreToolUse check on
# Edit|Write|MultiEdit that judges a write before it lands. The runner the
# settings entry dispatches through owns the exit-code safety; this body owns
# only the project's checks.
#
# Exit 0 allows the write. Exit 2 blocks it, and the plain-text reason on
# stderr reaches Claude. Any other code is a non-blocking error and the write
# proceeds, so a finding that should not block exits 0 with a `systemMessage`
# in stdout JSON instead.
#
# The file on disk is the one before the edit. What the write would put there
# is in `tool_input`: `content` for Write, `old_string` and `new_string` for
# Edit. Stdin is read once, into INPUT, and every later read pipes from it.
# `file_path` is absolute, so FILE is that path relative to the project root,
# which the runner made the working directory.
#
# Fail open: a missing jq, an empty path, a path outside the project, or a path
# with `..` allows the write. A broken check must never leave the session
# unable to write — fix the check.
set -uo pipefail

command -v jq >/dev/null 2>&1 || exit 0

INPUT=$(cat)
FILE=$(printf '%s' "$INPUT" | jq -r '.tool_input.file_path // ""' 2>/dev/null) || exit 0

[[ -z "$FILE" || "$FILE" == *..* || "$FILE" != "$PWD"/* ]] && exit 0
FILE=${FILE#"$PWD"/}

# --- project checks: each prints its reason to stderr and exits 2 ---

# Example: freeze the body of a document whose lifecycle status is terminal.
# Adapt the paths and the status values to the project's own lifecycle.
#
# if [[ "$FILE" == docs/* && -f "$FILE" ]]; then
#   STATUS=$(sed -n 's/^status:[[:space:]]*//p' "$FILE" | head -1 | tr -d '[:space:]')
#   case "$STATUS" in
#     superseded|archived|abandoned)
#       echo "${FILE} has terminal status '${STATUS}' — its body is frozen." >&2
#       exit 2 ;;
#   esac
# fi

exit 0
