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
#
# Each report names a command to run, so every path in one is printed through
# `%q` — the shell's own escaping, which bash 3.2 and 5.x spell alike. Bare, a
# path holding a space reaches `git config` as a value followed by a pattern:
# it stores the first word, exits 0, and leaves this printing the same advice
# every session after.
hooks=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P) || hooks=
present=()
for hook in pre-commit commit-msg; do
  [[ -n "$hooks" && -e "${hooks}/${hook}" ]] && present+=("$hook")
done
# A git too old to know `--git-path` echoes the flag onto stdout, reads the
# operand as the `hooks` directory beside it, and still exits 0 — so the answer
# is held against the flag this asked with before it is read as a path. The
# comparison is that exact string rather than the answer's shape: a repository
# whose own path holds a newline gives a two-line answer that is a path, and
# rejecting it by shape would leave a genuinely unarmed clone unreported.
# `--git-path` honours `core.hooksPath`, and git rewrites a relative one to
# answer to the working directory.
if [[ ${#present[@]} -gt 0 ]] &&
  active=$(git rev-parse --git-path hooks 2>/dev/null) &&
  [[ -n "$active" && "${active%%$'\n'*}" != "--git-path" ]]; then
  root=$(git rev-parse --show-toplevel 2>/dev/null) &&
    root=$(CDPATH='' cd -- "$root" 2>/dev/null && pwd -P) || root=
  if [[ "$active" != /* ]]; then
    # git rewrites a relative answer to answer to the working directory, but
    # only while that directory lies inside the work tree. Asked from outside
    # one — an exported `GIT_WORK_TREE` — it hands back the configured value
    # unchanged, and that answers to the work tree's root.
    here=$(pwd -P)
    base="$here"
    [[ -n "$root" && "$here" != "$root" && "$here" != "$root"/* ]] && base="$root"
    active="${base}/${active}"
  fi
  # An absent directory keeps its raw path: it is where git looks, and it holds
  # no hook, which is the state worth naming rather than resolving away.
  armed=$(CDPATH='' cd -- "$active" 2>/dev/null && pwd -P) || armed="$active"
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
    # git resolves a relative `core.hooksPath` against each work tree's own
    # root, so the relative form arms every linked worktree of this clone on
    # its own copy and survives the clone being moved; an absolute one aims
    # all of them at the tree this ran in.
    value="$hooks"
    [[ -n "$root" && "$hooks" == "$root"/* ]] && value="${hooks#"${root}"/}"
    printf 'Versioned git hooks are not armed: git runs hooks from %s.\nThis clone arms them with `git config%s core.hooksPath %s`.\n' \
      "$armed" "$scope" "$(printf '%q' "$value")"
  else
    # Absolute, because the command is read where the session stands rather
    # than here: the hook wrapper enters `CLAUDE_PROJECT_DIR`, which a monorepo
    # puts below the work-tree root that a relative path would answer to.
    quoted=""
    for hook in "${present[@]}"; do
      [[ -x "${hooks}/${hook}" ]] || quoted+=" $(printf '%q' "${hooks}/${hook}")"
    done
    if [[ -n "$quoted" ]]; then
      printf 'git runs hooks from here, and the executable bit is missing from%s, which git requires before it runs a hook.\nThis clone sets it with `chmod +x%s`.\n' \
        "$quoted" "$quoted"
    fi
  fi
fi
