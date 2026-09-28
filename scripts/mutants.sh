#!/usr/bin/env bash
# Run cargo-mutants with a ceiling on what one mutant may hold.
#
# A mutant is a deliberate defect, so a loop that allocates per turn becomes a
# loop that allocates without end. cargo-mutants bounds a mutant by wall clock
# and not by memory, and macOS honours no `ulimit -v`, so nothing between the
# two stops it: one run reached 780 GB of address space and put the machine
# into swap before its timeout came round.
#
# The watch walks the run's own process tree. cargo-mutants builds and tests
# each mutant in a copy of the source under `$TMPDIR`, so a watch keyed on the
# target directory sees none of them, and one keyed on a name would reach a
# build running beside this one.
#
# `ps` reports resident pages, so memory already compressed or swapped does not
# count toward the ceiling. Keep the ceiling well under free RAM or a runaway
# is swapped faster than it is seen.
#
# Arguments are passed through. `MUTANTS_CEILING_MB` sets the ceiling.
set -Eeuo pipefail

readonly CEILING_MB="${MUTANTS_CEILING_MB:-4096}"

BOLD='' RED='' RESET=''
if [ -t 2 ] && [ -z "${NO_COLOR:-}" ]; then
  BOLD=$'\033[1m' RED=$'\033[31m' RESET=$'\033[0m'
fi

step() { printf '%s==>%s %s\n' "$BOLD" "$RESET" "$*" >&2; }
warn() { printf '%s!%s   %s\n' "$RED" "$RESET" "$*" >&2; }

command -v cargo-mutants >/dev/null 2>&1 \
  || { warn "cargo-mutants is not installed"; exit 1; }

descendants() {
  local parent=$1 child
  for child in $(pgrep -P "$parent" 2>/dev/null || true); do
    printf '%s\n' "$child"
    descendants "$child"
  done
}

watch_ceiling() {
  local root=$1 pid rss
  while kill -0 "$root" 2>/dev/null; do
    for pid in $(descendants "$root"); do
      rss=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ')
      if [ -n "$rss" ] && [ "$rss" -gt $((CEILING_MB * 1024)) ]; then
        warn "mutant process ${pid} passed ${CEILING_MB} MB and was killed"
        kill -9 "$pid" 2>/dev/null || true
      fi
    done
    sleep 1
  done
}

# Job control gives the run a process group of its own, so the group id names
# this run and nothing beside it. Signalling the run alone leaves the mutant
# it was testing running and no longer watched: the watch ends with its root,
# and a mutant that allocates without end has nothing left to stop it.
set -m

# The group reaches what inherited it and the walk reaches what did not:
# `cargo-nextest` makes itself a group leader, so the tests it runs and the
# compiler runs under them sit outside the run's own group. The walk is taken
# before anything is signalled, because a process whose parent has gone is
# reparented to init and no longer answers to a walk of the tree it was in.
cleanup() {
  local held
  held=$(descendants "$RUN")
  kill "$WATCHER" 2>/dev/null || true
  kill -- "-$RUN" 2>/dev/null || true
  # shellcheck disable=SC2086 # a pid list, split on purpose
  kill $held "$RUN" 2>/dev/null || true
}

cargo mutants "$@" &
readonly RUN=$!
step "ceiling ${CEILING_MB} MB, watching the tree under ${RUN}"
watch_ceiling "$RUN" &
readonly WATCHER=$!
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

wait "$RUN"
