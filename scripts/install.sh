#!/usr/bin/env bash
# Install the harnex oracle, the binary the plugin's skills and commands call.
#
# The plugin does not install it. A plugin being enabled is not consent to put
# an executable on the machine, so this is run deliberately and says what it
# did.
set -Eeuo pipefail

readonly REPO_URL="https://github.com/junyeong-ai/harnex"
readonly BINARY="harness"
readonly CRATE_PATH="crates/harness-cli"

BOLD='' DIM='' RED='' GREEN='' RESET=''
if [ -t 2 ] && [ -z "${NO_COLOR:-}" ]; then
  BOLD=$'\033[1m' DIM=$'\033[2m' RED=$'\033[31m' GREEN=$'\033[32m' RESET=$'\033[0m'
fi

log()  { printf '%s\n' "$*" >&2; }
step() { printf '%s==>%s %s\n' "$BOLD" "$RESET" "$*" >&2; }
warn() { printf '%s!%s   %s\n' "$RED" "$RESET" "$*" >&2; }
die()  { printf '%serror:%s %s\n' "$RED" "$RESET" "$*" >&2; exit 1; }

on_error() { die "install failed at line $1"; }
trap 'on_error $LINENO' ERR

usage() {
  cat >&2 <<EOF
${BOLD}harnex install${RESET}

  scripts/install.sh [options]

Builds and installs the ${BINARY} binary with cargo. Run it from a clone, or
from anywhere with --from-git.

Options:
  --from-git        build from ${REPO_URL} instead of the working tree
  --rev <ref>       with --from-git, the branch, tag or commit to build
  --bin-dir <path>  where to install (default: cargo's own bin directory)
  --check           report what is installed and exit without changing anything
  -h, --help        this

Environment:
  CARGO_HOME        respected for the default install directory
EOF
}

FROM_GIT=0
REV=""
BIN_DIR=""
CHECK_ONLY=0

while [ $# -gt 0 ]; do
  case "$1" in
    --from-git) FROM_GIT=1; shift ;;
    --rev)      REV="${2:?--rev needs a ref}"; shift 2 ;;
    --bin-dir)  BIN_DIR="${2:?--bin-dir needs a path}"; shift 2 ;;
    --check)    CHECK_ONLY=1; shift ;;
    -h|--help)  usage; exit 0 ;;
    *)          usage; die "unknown option: $1" ;;
  esac
done

[ -n "$REV" ] && [ "$FROM_GIT" -eq 0 ] && die "--rev applies to --from-git"

have() { command -v "$1" >/dev/null 2>&1; }

# The version the workspace declares, so this script never carries a second
# copy of it to drift from.
required_rust() {
  local root="$1"
  [ -f "$root/Cargo.toml" ] || return 0
  sed -n 's/^rust-version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$root/Cargo.toml" | head -1
}

# Numeric comparison, so 1.100 sorts above 1.97 rather than below it.
version_at_least() {
  local have="$1" want="$2" h w
  IFS=. read -r h w _ <<<"$have";   local h1=${h:-0} h2=${w:-0}
  IFS=. read -r h w _ <<<"$want";   local w1=${h:-0} w2=${w:-0}
  [ "$h1" -gt "$w1" ] && return 0
  [ "$h1" -lt "$w1" ] && return 1
  [ "$h2" -ge "$w2" ]
}

report_installed() {
  if have "$BINARY"; then
    printf '%s%s%s at %s\n' "$GREEN" "$($BINARY --version 2>/dev/null || echo "$BINARY")" \
      "$RESET" "$(command -v "$BINARY")" >&2
    return 0
  fi
  log "${DIM}${BINARY} is not on PATH${RESET}"
  return 1
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ "$CHECK_ONLY" -eq 1 ]; then
  report_installed || exit 1
  exit 0
fi

step "checking prerequisites"
have cargo || die "cargo is not installed — see https://rustup.rs, then re-run this"

CARGO_VERSION="$(cargo --version | awk '{print $2}')"
WANT="$(required_rust "$ROOT")"
if [ -n "$WANT" ] && ! version_at_least "$CARGO_VERSION" "$WANT"; then
  die "cargo $CARGO_VERSION is older than the $WANT this workspace declares — run: rustup update"
fi
log "${DIM}cargo $CARGO_VERSION${RESET}"

if [ -z "$BIN_DIR" ]; then
  BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
fi

INSTALL=(cargo install --locked --force --root "${BIN_DIR%/bin}")
if [ "$FROM_GIT" -eq 1 ]; then
  step "building from $REPO_URL${REV:+ @ $REV}"
  INSTALL+=(--git "$REPO_URL")
  [ -n "$REV" ] && INSTALL+=(--rev "$REV")
  INSTALL+=("harness-cli")
else
  [ -f "$ROOT/$CRATE_PATH/Cargo.toml" ] ||
    die "$ROOT is not a harnex clone — use --from-git, or run this from inside one"
  step "building from $ROOT"
  INSTALL+=(--path "$ROOT/$CRATE_PATH")
fi

"${INSTALL[@]}" || die "cargo install failed — the output above says why"

step "verifying"
INSTALLED="${BIN_DIR%/}/$BINARY"
[ -x "$INSTALLED" ] || die "cargo reported success but $INSTALLED is not executable"
"$INSTALLED" --version >/dev/null || die "$INSTALLED did not run"

printf '\n%s%s%s → %s\n' "$GREEN" "$("$INSTALLED" --version)" "$RESET" "$INSTALLED" >&2

if ! have "$BINARY"; then
  warn "$BIN_DIR is not on PATH — add it, or the plugin will report the oracle as missing:"
  log "    export PATH=\"$BIN_DIR:\$PATH\""
fi

cat >&2 <<EOF

${DIM}next${RESET}
  harness --help                  what the oracle answers
  harness check                   run every gate this project declares
  /plugin marketplace add junyeong-ai/harnex
  /plugin install harnex@harnex   the skill and commands that call it

${DIM}to remove:${RESET} cargo uninstall --root "${BIN_DIR%/bin}" harness-cli
EOF
