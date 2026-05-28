#!/usr/bin/env bash
# PreToolUse(Edit|Write): check the target file against write-time
# conventions BEFORE the write completes. Exit 0 = allow, exit 2 = block
# (stderr reason feeds back to Claude). Pure bash + jq.
#
# Add project conventions as checks below. Each check that fails prints a
# plain-text reason to stderr and exits 2.
set -uo pipefail

command -v jq >/dev/null 2>&1 || exit 0

INPUT=$(cat)
FILE=$(echo "$INPUT" | jq -r '.tool_input.file_path // ""' 2>/dev/null) || exit 0

[[ -z "$FILE" || "$FILE" == *..* ]] && exit 0

# --- project conventions (add checks below) ---

# Example: protect files with terminal lifecycle status from body edits.
# Uncomment and adapt the pattern + status values to the project's
# document lifecycle.
#
# if [[ "$FILE" == docs/* || "$FILE" == specs/* ]] && [[ -f "$FILE" ]]; then
#   STATUS=$(head -20 "$FILE" | grep -oP '(?<=^status:\s).+' | tr -d '[:space:]')
#   case "$STATUS" in
#     superseded|archived|deprecated|abandoned)
#       echo "${FILE} has terminal status '${STATUS}' — body edits are frozen." >&2
#       exit 2 ;;
#   esac
# fi

exit 0
