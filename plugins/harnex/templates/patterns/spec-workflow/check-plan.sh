#!/usr/bin/env bash
# git pre-commit arm — the review grammar's commit floor.
#
# For every spec whose plan OR spec is staged, `harnex plan audit` holds what
# is being committed to the contract the gates wrote: no open Critical/Blocker
# row, no row deleted, reworded or downgraded instead of gaining its terminal
# disposition, no approval recorded over what its gate still counts against it
# — an open Blocker, or an acceptance criterion nothing measured — no rows
# landing without the round that found them or past what that commit's own
# rounds counted at that rank, no round past the gate's budget, and a decision log
# that only ever appends. A commit that leaves neither
# document retires the spec and is held to none of it. The staged content
# is what is judged — the worktree may be further along — and HEAD is the
# baseline both append-only contracts are held against. Paths are read
# NUL-delimited with renames split into delete + add: a rename that also
# deletes a row, or a spec directory named with a space or outside ASCII,
# must reach the audit like any other path.
#
# Fail-open on a missing binary and on a runtime failure of the tool itself;
# only findings block. Escape hatch via HARNEX_SKIP_PLANCHECK=1 when the
# operator has decided the state may land as it stands. Set, it says so on the
# commit it skips, and the block message below names no way around itself: the
# loop this floor bounds is what reads it first.
set -uo pipefail

if [[ "${HARNEX_SKIP_PLANCHECK:-}" == "1" ]]; then
  echo "[harnex] review floor skipped by operator override — this commit was not judged." >&2
  exit 0
fi

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
  spec="${f%plan.md}spec.md"
  # A commit that leaves neither document is the spec's retirement, not its
  # record disappearing: there is nothing left to hold, and the rows it held
  # went with the unit that owned them. Judged from the index rather than from
  # the diff, so a spec removed over several commits reaches the same answer.
  if ! git cat-file -e ":$f" 2>/dev/null && ! git cat-file -e ":$spec" 2>/dev/null; then
    rm -rf "$tmp"
    continue
  fi
  # Rounds one gate may spend on one spec before reaching the number is a
  # report. Raise it for a genuinely large scope; a review that needs many
  # more is naming a unit too large to finish as one. The gate list is this
  # workflow's own: the budget is per gate, so a firing under a name nothing
  # declares carries a budget of its own. Add a gate here when the workflow
  # gains one.
  args=(--plan "$f" --max-rounds 5 --gates clarify,design_review,review,acceptance,resume)
  git show ":$f" >"$tmp/$f" 2>/dev/null || rm -f "$tmp/$f"
  # The four inputs are always supplied, and absent is spelled as empty. A
  # staged plan whose spec is not in the index would otherwise skip every
  # check the log carries — the round record among them — and an empty one
  # makes the missing ledger the finding it is. A path HEAD does not carry is
  # committed to nothing, which is a baseline of no rows and no records: the
  # first commit of a spec carries rows only if a pass found them.
  git show ":$spec" >"$tmp/$spec" 2>/dev/null || : >"$tmp/$spec"
  args+=(--spec "$spec")
  git show "HEAD:$f" >"$tmp/$f.baseline" 2>/dev/null || : >"$tmp/$f.baseline"
  args+=(--baseline "$f.baseline")
  git show "HEAD:$spec" >"$tmp/$spec.baseline" 2>/dev/null || : >"$tmp/$spec.baseline"
  args+=(--baseline-spec "$spec.baseline")

  out=$(cd "$tmp" && harnex plan audit "${args[@]}" 2>/dev/null)
  code=$?
  rm -rf "$tmp"
  case $code in
  0) ;;
  1)
    echo "[harnex] $f fails the review floor — commit blocked." >&2
    echo "$out" >&2
    echo "         Landing it anyway is the operator's call; this hook says how." >&2
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
