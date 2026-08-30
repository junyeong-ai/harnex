#!/usr/bin/env bash
# git pre-commit arm — the review grammar's commit floor.
#
# For every staged spec plan, `harnex plan audit` holds what is being
# committed to the contract the gates wrote: no open Critical/Blocker row, no
# row deleted or reworded instead of gaining its terminal disposition, and a
# decision log whose counts converge. The staged content is what is judged —
# the worktree may be further along — and HEAD is the baseline the append-only
# contract is held against.
#
# Fail-open on a missing binary and on a runtime failure of the tool itself;
# only findings block. Escape hatch via HARNEX_SKIP_PLANCHECK=1 when the
# operator has decided the state may land as it stands.
set -uo pipefail

[[ "${HARNEX_SKIP_PLANCHECK:-}" == "1" ]] && exit 0

command -v harnex >/dev/null 2>&1 || {
  echo "[harnex] harnex not installed — plan audit skipped." >&2
  exit 0
}

PLAN_GLOB="specs/*/plan.md"

STAGED=$(git diff --cached --name-only --diff-filter=ACMRD) || exit 0

status=0
for f in $STAGED; do
  # shellcheck disable=SC2254 -- the glob is the match
  case "$f" in
  $PLAN_GLOB) ;;
  *) continue ;;
  esac

  # The staged content is judged from a temp tree that mirrors the repo's
  # relative paths, so findings name the file the operator knows.
  tmp=$(mktemp -d) || exit 0
  mkdir -p "$tmp/$(dirname "$f")"
  args=(--plan "$f")
  git show ":$f" >"$tmp/$f" 2>/dev/null || rm -f "$tmp/$f"
  spec="${f%plan.md}spec.md"
  if git show ":$spec" >"$tmp/$spec" 2>/dev/null; then
    args+=(--spec "$spec")
  fi
  if git show "HEAD:$f" >"$tmp/$f.baseline" 2>/dev/null; then
    args+=(--baseline "$f.baseline")
  fi

  out=$(cd "$tmp" && harnex plan audit "${args[@]}" 2>/dev/null)
  code=$?
  rm -rf "$tmp"
  case $code in
  0) ;;
  1)
    echo "[harnex] $f fails the review floor — commit blocked." >&2
    echo "$out" >&2
    echo "         Operator override: HARNEX_SKIP_PLANCHECK=1 git commit ..." >&2
    status=1
    ;;
  *)
    echo "[harnex] plan audit could not run on $f (exit $code) — skipped." >&2
    ;;
  esac
done
exit $status
