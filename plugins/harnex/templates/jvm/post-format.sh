#!/usr/bin/env bash
# PostToolUse(Edit|Write): format the edited file. Advisory, always exits 0.
# file_path is parsed from stdin with jq — a JVM repo bundles neither a
# Python nor a Node interpreter, so jq is the canonical shell tool; if it is
# absent the hook skips rather than hand-roll fragile JSON parsing.
# Path traversal and non-existent files are skipped.
#
# The formatter is invoked per file and never through the build tool. A
# `gradlew spotlessApply` / `mvn spotless:apply` starts a JVM and a daemon
# and formats the whole project, which exceeds the hook timeout on every
# edit; the standalone CLIs finish in the budget a per-edit hook has.
# Each arm skips silently when its formatter is absent, so a repo that
# formats only one of the two languages is not penalised for the other.
set -uo pipefail

command -v jq >/dev/null 2>&1 || exit 0

FILE=$(jq -r '.tool_input.file_path // ""' 2>/dev/null) || exit 0

[[ -z "$FILE" || "$FILE" == *..* || ! -f "$FILE" ]] && exit 0

case "$FILE" in
  *.java)      command -v google-java-format >/dev/null 2>&1 && google-java-format -i "$FILE" >/dev/null 2>&1 || true ;;
  *.kt|*.kts)  command -v ktlint >/dev/null 2>&1 && ktlint -F "$FILE" >/dev/null 2>&1 || true ;;
esac
exit 0
