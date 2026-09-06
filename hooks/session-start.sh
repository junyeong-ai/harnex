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

# Whether the versioned git hooks beside this one will run. Two things decide
# that and each fails silently: `core.hooksPath` lives in the clone's own
# config and is never cloned, and git skips a hook carrying no executable bit,
# saying so only on the stderr of whichever git command was run. A fresh
# checkout, or a copy that dropped the mode, then commits past every gate they
# hold — armed and absent reading alike, which is the shape a harness cannot
# afford. Both are reported by exception, and here rather than in `harnex
# check`: the answer is a property of this machine, and a gate whose verdict
# moves without the tree fails a tree nothing changed.
#
# The git hooks the scaffold ships are named rather than the directory
# scanned, and `scaffold_git_hooks_match_the_probe` holds the pair to the
# manifest — a third one shipping fails the build instead of going unwatched.
hooks=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P) || hooks=
present=()
for hook in pre-commit commit-msg; do
  [[ -n "$hooks" && -e "${hooks}/${hook}" ]] && present+=("$hook")
done
if [[ ${#present[@]} -gt 0 ]] &&
  active=$(git rev-parse --git-path hooks 2>/dev/null) &&
  [[ -n "$active" && "$active" != *$'\n'* ]]; then
  # `rev-parse` echoes a flag it does not know onto stdout and still exits 0,
  # so the answer is held to the one line the contract promises before it is
  # read as a path: a version predating one would otherwise be taken for a
  # directory named after it. `--git-path` alone is asked because it predates
  # `core.hooksPath` itself, so a repository that can be in this state has a
  # git that knows it. Its answer is relative to the working directory.
  [[ "$active" == /* ]] || active="${PWD}/${active}"
  # An absent directory keeps its raw path: it is where git looks, and it holds
  # no hook, which is the state worth naming rather than resolving away.
  armed=$(CDPATH='' cd -- "$active" 2>/dev/null && pwd -P) || armed="$active"
  root=$(git rev-parse --show-toplevel 2>/dev/null) &&
    root=$(CDPATH='' cd -- "$root" 2>/dev/null && pwd -P) || root=
  here="$hooks"
  [[ -n "$root" && "$hooks" == "$root"/* ]] && here="${hooks#"${root}"/}"
  if [[ "$armed" != "$hooks" ]]; then
    # Which scope holds the setting decides which scope can change it: a
    # worktree-scoped value shadows the shared one, so a command naming the
    # shared scope would run, report success, and leave this reading the same.
    # `--worktree` is `--local` wearing another name until `worktreeConfig` is
    # enabled, so asking it alone reads any ordinary local value — the shape
    # every hook manager leaves behind — as worktree-scoped. Both are asked.
    scope=""
    if [[ "$(git config --bool --get extensions.worktreeConfig 2>/dev/null)" == "true" ]] &&
      git config --worktree --get core.hooksPath >/dev/null 2>&1; then
      scope=" --worktree"
    fi
    printf 'Versioned git hooks are not armed: git runs hooks from %s.\nThis clone arms them with `git config%s core.hooksPath %s`.\n' \
      "$armed" "$scope" "$here"
  else
    inert=()
    for hook in "${present[@]}"; do
      [[ -x "${hooks}/${hook}" ]] || inert+=("${here}/${hook}")
    done
    if [[ ${#inert[@]} -gt 0 ]]; then
      # Each path is quoted in both halves. They are relative to the work tree
      # only while the hooks live under it, and the absolute form a detached
      # directory falls back to can carry a space — which splits the command
      # and reads as one path in the sentence.
      quoted=""
      for path in "${inert[@]}"; do
        quoted+=" '${path}'"
      done
      printf 'git runs hooks from here, and the executable bit is missing from%s, which git requires before it runs a hook.\nThis clone sets it with `chmod +x%s`.\n' \
        "$quoted" "$quoted"
    fi
  fi
fi
