#!/usr/bin/env bash
# SessionStart: inject branch + uncommitted count + recent commits as
# additionalContext. JSON is built by node (present in a TS repo) so
# branch/commit text is escaped correctly. Falls back to a minimal envelope.
set -uo pipefail

BRANCH=$(git branch --show-current 2>/dev/null || echo unknown)
CHANGES=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
COMMITS=$(git log --oneline -3 2>/dev/null || echo "no commits")

H_BRANCH="$BRANCH" H_CHANGES="$CHANGES" H_COMMITS="$COMMITS" node -e '
const ctx = `Branch: ${process.env.H_BRANCH}\nUncommitted files: ${process.env.H_CHANGES}\nRecent commits:\n${process.env.H_COMMITS}`;
process.stdout.write(JSON.stringify({ additionalContext: ctx }));
' 2>/dev/null || echo "{\"additionalContext\":\"Branch: ${BRANCH}\"}"
