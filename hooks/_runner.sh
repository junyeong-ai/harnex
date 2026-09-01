#!/usr/bin/env bash
# Claude Code hook wrapper. Anchors cwd at the project root so a verifier's
# relative paths resolve the same wherever Claude fired the hook from,
# refuses path traversal in the verifier name, and dispatches by extension.
# Fails open on environment drift — a broken toolchain must never block an
# edit, and the gates remain the structural failure surface.
#
# Three anchors, none of them the working directory. Claude Code fires hooks
# from wherever the session is, so asking git where the root is FROM THERE
# answers about that directory rather than about this harness: inside a
# submodule or any nested checkout it names the inner repository, and every
# verifier silently stops being found.
#
# In order: the runtime states the project in CLAUDE_PROJECT_DIR, which is how
# the shipped wiring launches this file and so cannot disagree with it; failing
# that, git is asked about the directory THIS FILE sits in, which is stable
# wherever the session wandered and is right for a harness kept below the root;
# failing that, the parent, which is the scaffolded layout. Only the last two
# are reached by a hand-run hook, and a project that keeps no git still gets
# one, so its hooks run.
#
# One wrapper serves every language. What differs per ecosystem is which
# interpreter a verifier needs, so each non-shell arm probes its own and
# skips when it is absent. A `.sh` verifier that shells out to a formatter
# probes that formatter itself, which is why this file probes nothing for
# the shell arm: gating on a toolchain the verifier never calls would skip a
# working hook for an unrelated reason.
#
# A missing verifier is a separate question from a missing toolchain, and it
# is checked once for every arm rather than per interpreter: dispatching to a
# file that is not there ends the hook non-zero on every edit (127 from bash,
# an interpreter error from the others), which is the one outcome this
# wrapper exists to prevent. `harnex audit` reports the absence as coverage.
set -euo pipefail

HOOKS="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)" || { echo "[harnex-skipped: cannot locate the hooks directory]" >&2; exit 0; }
ROOT="${CLAUDE_PROJECT_DIR:-$(git -C "${HOOKS}" rev-parse --show-toplevel 2>/dev/null || echo "${HOOKS%/*}")}"
# A root is an absolute path. Relative, it would resolve against the directory
# the hook happened to fire in — the anchor this wrapper exists to stop reading.
case "$ROOT" in
  /*) ;;
  *) echo "[harnex-skipped: project root is not an absolute path: ${ROOT}]" >&2; exit 0 ;;
esac
# Explicit, because the two shells disagree on the default: under `set -e` an
# unguarded failure ends the hook non-zero, which is the blocked edit this
# wrapper exists to prevent.
cd "${ROOT}" 2>/dev/null || { echo "[harnex-skipped: cannot enter project root: ${ROOT}]" >&2; exit 0; }

[[ $# -eq 0 ]] && { echo "[harnex-skipped: no script argument]" >&2; exit 0; }
SCRIPT="$1"; shift

case "$SCRIPT" in
  *..*) echo "[harnex-skipped: path traversal refused: $SCRIPT]" >&2; exit 0 ;;
esac

# Beside this wrapper, never under the root: the two are the same directory in
# a scaffolded harness, and where they are not, the verifiers are still here.
VERIFIER="${HOOKS}/${SCRIPT}"
[[ -f "${VERIFIER}" ]] || { echo "[harnex-skipped: verifier not found: ${VERIFIER}]" >&2; exit 0; }

case "$SCRIPT" in
  *.sh)
    exec bash "${VERIFIER}" "$@"
    ;;
  *.py)
    # `--frozen` never mutates the lockfile: re-locking as a side effect of a
    # hook firing is a surprise, so drift skips and the developer re-syncs
    # deliberately.
    uv run --frozen python -c "" 2>/dev/null || { echo "[harnex-skipped: uv env unavailable — run 'uv sync']" >&2; exit 0; }
    exec uv run --frozen python "${VERIFIER}" "$@"
    ;;
  *.ts|*.js|*.mjs)
    command -v node >/dev/null 2>&1 || { echo "[harnex-skipped: node not found]" >&2; exit 0; }
    exec node "${VERIFIER}" "$@"
    ;;
  *)
    echo "[harnex-skipped: unsupported verifier extension: $SCRIPT]" >&2; exit 0
    ;;
esac
