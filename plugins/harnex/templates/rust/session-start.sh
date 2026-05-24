#!/usr/bin/env bash
# SessionStart: inject branch + uncommitted count + recent commits as
# additionalContext. A compiled Rust repo has no bundled JSON interpreter,
# so jq (the canonical shell JSON tool) builds the payload with correct
# escaping of branch/commit text. If jq is absent, emit nothing — context is
# advisory, and a hand-built fallback could not escape the same values safely.
set -uo pipefail

command -v jq >/dev/null 2>&1 || exit 0

BRANCH=$(git branch --show-current 2>/dev/null || echo unknown)
CHANGES=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
COMMITS=$(git log --oneline -3 2>/dev/null || echo "no commits")

jq -nc \
  --arg branch "$BRANCH" \
  --arg changes "$CHANGES" \
  --arg commits "$COMMITS" \
  '{additionalContext: ("Branch: " + $branch + "\nUncommitted files: " + $changes + "\nRecent commits:\n" + $commits)}' \
  2>/dev/null || exit 0
