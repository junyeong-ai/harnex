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

# Whether the versioned git hooks beside this one will run. `core.hooksPath`
# lives in the clone's own config and is never cloned, so a fresh checkout
# commits past every gate they hold and nothing else reads that — the shape a
# harness cannot afford, because armed and absent read alike. Reported by
# exception, and here rather than in `harnex check`: the answer is a property
# of this machine, and a gate whose verdict moves without the tree fails a
# tree nothing changed. `git rev-parse` resolves the setting rather than this
# reading it, so a relative path answers per worktree exactly as git will.
hooks=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P) || hooks=
if [[ -n "$hooks" && -e "${hooks}/pre-commit" ]] &&
  active=$(git rev-parse --path-format=absolute --git-path hooks 2>/dev/null); then
  # An absent directory keeps its raw path: it is where git looks, and it holds
  # no hook, which is the state worth naming rather than resolving away.
  armed=$(CDPATH='' cd -- "$active" 2>/dev/null && pwd -P) || armed="$active"
  if [[ "$armed" != "$hooks" ]]; then
    root=$(git rev-parse --show-toplevel 2>/dev/null) &&
      root=$(CDPATH='' cd -- "$root" 2>/dev/null && pwd -P) || root=
    value="$hooks"
    [[ -n "$root" && "$hooks" == "$root"/* ]] && value="${hooks#"${root}"/}"
    # Which scope holds the setting decides which scope can change it: a
    # worktree-scoped value shadows the shared one, so the command naming the
    # shared scope would run, report success, and leave this reading the same.
    # Advice that silently does nothing is the shape this probe exists against,
    # so git is asked where the value lives rather than the common case assumed.
    scope=""
    git config --worktree --get core.hooksPath >/dev/null 2>&1 && scope=" --worktree"
    printf 'Versioned git hooks are not armed: git runs hooks from %s.\nThis clone arms them with `git config%s core.hooksPath %s`.\n' \
      "$armed" "$scope" "$value"
  fi
fi
