#!/usr/bin/env bash
# git pre-commit arm — the review grammar's commit floor.
#
# For every spec whose plan OR spec is staged, `harnex plan audit` holds what
# is being committed to the contract the gates wrote: no open Critical/Blocker
# row, no row deleted, reworded or downgraded instead of gaining its terminal
# disposition, no approval recorded over what its gate still counts against it
# — an open Blocker, or an acceptance criterion nothing measured — and a
# decision log that only ever appends. The staged content
# is what is judged — the worktree may be further along — and HEAD is the
# baseline both append-only contracts are held against. Paths are read
# NUL-delimited with renames split into delete + add: a rename that also
# deletes a row, or a spec directory named with a space or outside ASCII,
# must reach the audit like any other path.
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
SPEC_GLOB="specs/*/spec.md"

status=0
while IFS= read -r -d '' f; do
  tmp=$(mktemp -d) || {
    echo "[harnex] mktemp failed — plan audit skipped." >&2
    exit "$status"
  }
  # The staged content is judged from a temp tree that mirrors the repo's
  # relative paths, so findings name the file the operator knows.
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
  if git show "HEAD:$spec" >"$tmp/$spec.baseline" 2>/dev/null; then
    args+=(--baseline-spec "$spec.baseline")
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
done < <(
  # A staged spec.md alone is enough to rewrite the decision log, so both
  # artifacts trigger, mapped to their spec's plan path and de-duplicated —
  # one audit per spec, whichever half the commit stages.
  git -c core.quotePath=off diff --cached --name-only -z --no-renames --diff-filter=ACMRD |
    while IFS= read -r -d '' p; do
      # shellcheck disable=SC2254 -- the globs are the match
      case "$p" in
      $PLAN_GLOB) printf '%s\0' "$p" ;;
      $SPEC_GLOB) printf '%s\0' "${p%spec.md}plan.md" ;;
      esac
    done | sort -zu
)
exit $status
