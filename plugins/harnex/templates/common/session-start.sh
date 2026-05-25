#!/usr/bin/env bash
# SessionStart: surface branch + uncommitted count + recent commits as
# context. SessionStart delivers a hook's plain stdout to Claude directly
# (per the hooks spec), so no JSON envelope — and no language-specific JSON
# tool — is needed: print the facts as text. Fail-open: a git error in a
# non-repo or detached state emits nothing rather than failing the session.
set -uo pipefail

branch=$(git branch --show-current 2>/dev/null || echo unknown)
changes=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
commits=$(git log --oneline -3 2>/dev/null || echo "no commits")

printf 'Branch: %s\nUncommitted files: %s\nRecent commits:\n%s\n' \
  "$branch" "$changes" "$commits"
