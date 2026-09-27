#!/usr/bin/env bash
# Run cargo-mutants with a ceiling on what one mutant may hold.
#
# A mutant is a deliberate defect, so a loop that allocates per turn becomes a
# loop that allocates without end. cargo-mutants bounds a mutant by wall clock
# and not by memory, and macOS honours no `ulimit -v`, so nothing between the
# two stops it: one run reached 780 GB of address space and put the machine
# into swap before its timeout came round.
#
# Arguments are passed through. `MUTANTS_CEILING_MB` sets the ceiling per test
# process.
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

# The test binaries cargo-mutants builds are what grow. Matching on the target
# directory rather than on a crate name stays true as crates are added, and
# keeps the watch off a build of this repository running beside it.
readonly TARGET="${CARGO_TARGET_DIR:-$PWD/target}"

watch_ceiling() {
  local pid rss
  while :; do
    for pid in $(pgrep -f "^${TARGET}/" 2>/dev/null || true); do
      rss=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ')
      if [ -n "$rss" ] && [ "$rss" -gt $((CEILING_MB * 1024)) ]; then
        warn "mutant ${pid} passed ${CEILING_MB} MB and was killed"
        kill -9 "$pid" 2>/dev/null || true
      fi
    done
    sleep 1
  done
}

step "ceiling ${CEILING_MB} MB per test process, watching ${TARGET}"
watch_ceiling &
readonly WATCHER=$!
trap 'kill "$WATCHER" 2>/dev/null || true' EXIT

cargo mutants "$@"
