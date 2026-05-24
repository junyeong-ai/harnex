#!/usr/bin/env bash
# SessionStart: inject branch + uncommitted count + recent commits as
# additionalContext. JSON is built by python3 (present in a Python repo) via
# os.environ so branch/commit text is escaped correctly. Minimal fallback.
set -uo pipefail

BRANCH=$(git branch --show-current 2>/dev/null || echo unknown)
CHANGES=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
COMMITS=$(git log --oneline -3 2>/dev/null || echo "no commits")

H_BRANCH="$BRANCH" H_CHANGES="$CHANGES" H_COMMITS="$COMMITS" python3 -c "
import json, os
ctx  = 'Branch: ' + os.environ['H_BRANCH'] + '\n'
ctx += 'Uncommitted files: ' + os.environ['H_CHANGES'] + '\n'
ctx += 'Recent commits:\n' + os.environ['H_COMMITS']
print(json.dumps({'additionalContext': ctx}))
" 2>/dev/null || echo "{\"additionalContext\":\"Branch: ${BRANCH}\"}"
