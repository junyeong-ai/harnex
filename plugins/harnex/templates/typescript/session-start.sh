#!/usr/bin/env bash
# SessionStart: inject branch + uncommitted count + recent commits as
# additionalContext. node (present in a TS repo) builds the JSON so
# branch/commit text is escaped correctly. If node fails (pathological),
# emit nothing — context is advisory, and a hand-built fallback could not
# escape the same values safely.
set -uo pipefail

BRANCH=$(git branch --show-current 2>/dev/null || echo unknown)
CHANGES=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
COMMITS=$(git log --oneline -3 2>/dev/null || echo "no commits")

H_BRANCH="$BRANCH" H_CHANGES="$CHANGES" H_COMMITS="$COMMITS" node -e '
const ctx = `Branch: ${process.env.H_BRANCH}\nUncommitted files: ${process.env.H_CHANGES}\nRecent commits:\n${process.env.H_COMMITS}`;
process.stdout.write(JSON.stringify({ additionalContext: ctx }));
' 2>/dev/null || exit 0
