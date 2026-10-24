#!/usr/bin/env bash
# Install the harnex oracle, the binary the plugin's skills and commands call.
#
# The plugin does not install it. A plugin being enabled is not consent to put
# an executable on the machine, so this is run deliberately and says what it
# did.
#
# It takes the binary the release workflow built, verifies its checksum, and
# puts it in place. Building from source is the same install with --build.
set -Eeuo pipefail

readonly REPO="junyeong-ai/harnex"
readonly REPO_URL="https://github.com/$REPO"
readonly BINARY="harnex"
readonly CRATE_PATH="crates/harness-cli"

BOLD='' DIM='' RED='' GREEN='' RESET=''
if [ -t 2 ] && [ -z "${NO_COLOR:-}" ]; then
  BOLD=$'\033[1m' DIM=$'\033[2m' RED=$'\033[31m' GREEN=$'\033[32m' RESET=$'\033[0m'
fi

log()  { printf '%s\n' "$*" >&2; }
step() { printf '%s==>%s %s\n' "$BOLD" "$RESET" "$*" >&2; }
warn() { printf '%s!%s   %s\n' "$RED" "$RESET" "$*" >&2; }
die()  { printf '%serror:%s %s\n' "$RED" "$RESET" "$*" >&2; exit 1; }

# Inside a command substitution or subshell, a failure ends it with its status
# and prints nothing: the caller handles it, or fails in turn and is reported
# here, once. Bash leaves three shapes outside this, and the script holds none
# of them: a substitution in an argument drops its status; under a caller that
# handles its result, a helper's earlier failed step is run past, except in a
# command substitution on bash 3.2, which ends there; and bash 3.2 runs no trap
# when an unhandled `( … )` fails.
on_error() {
  local status=$?
  [ "$BASH_SUBSHELL" -eq 0 ] || exit "$status"
  die "install failed at line $1"
}
trap 'on_error $LINENO' ERR

usage() {
  cat >&2 <<EOF
${BOLD}harnex install${RESET}

  scripts/install.sh [options]
  curl -fsSL $REPO_URL/raw/main/scripts/install.sh | bash

Downloads the ${BINARY} binary this project releases for your platform,
verifies its checksum, and installs it. No Rust toolchain needed.

Options:
  --version <tag>   install this release instead of the latest (v1.2.3 or 1.2.3)
  --build           build from source with cargo instead of downloading
  --rev <ref>       with --build, the commit or tag to build
  --bin-dir <path>  where the binary lands (default: \$HOME/.local/bin)
  --check           report what is installed and exit without changing anything
  -h, --help        this

Environment:
  XDG_BIN_HOME      respected for the default install directory
EOF
}

VERSION=""
BUILD=0
REV=""
BIN_DIR=""
CHECK_ONLY=0

while [ $# -gt 0 ]; do
  case "$1" in
    --version)  VERSION="${2:?--version needs a tag}"; shift 2 ;;
    --build)    BUILD=1; shift ;;
    --rev)      REV="${2:?--rev needs a ref}"; shift 2 ;;
    --bin-dir)  BIN_DIR="${2:?--bin-dir needs a path}"; shift 2 ;;
    --check)    CHECK_ONLY=1; shift ;;
    -h|--help)  usage; exit 0 ;;
    *)          usage; die "unknown option: $1" ;;
  esac
done

[ -n "$REV" ] && [ "$BUILD" -eq 0 ] && die "--rev applies to --build"
# Every release is tagged v<version> (release.yml), and a pin's floor is
# written without the v, so either spelling names the same release.
case "$VERSION" in
  "" | v*) ;;
  *) VERSION="v$VERSION" ;;
esac
[ -n "$VERSION" ] && [ "$BUILD" -eq 1 ] && die "--version selects a release; --build selects a source tree"

have() { command -v "$1" >/dev/null 2>&1; }

# The clone this script lives in, if it lives in one. Piped from curl it does
# not, and every path below has to hold without it.
ROOT=""
SELF="${BASH_SOURCE[0]:-}"
if [ -n "$SELF" ]; then
  candidate="$(cd "$(dirname "$SELF")/.." 2>/dev/null && pwd)" || candidate=""
  [ -n "$candidate" ] && [ -f "$candidate/$CRATE_PATH/Cargo.toml" ] && ROOT="$candidate"
fi

# ── what this machine is ─────────────────────────────────────────────────────

# Linux ships musl, so one archive per architecture runs on any distribution
# rather than carrying the build runner's glibc floor.
host_target() {
  local os arch
  case "$(uname -s)" in
    Darwin) os=apple-darwin ;;
    Linux)  os=unknown-linux-musl ;;
    *) return 1 ;;
  esac
  case "$(uname -m)" in
    arm64 | aarch64) arch=aarch64 ;;
    x86_64 | amd64)  arch=x86_64 ;;
    *) return 1 ;;
  esac
  printf '%s-%s\n' "$arch" "$os"
}

# ── placement, shared by both paths ──────────────────────────────────────────

place() {
  local src="$1" dest="$BIN_DIR/$BINARY" tmp="$BIN_DIR/.$BINARY.$$"
  mkdir -p "$BIN_DIR"
  cp "$src" "$tmp"
  chmod 0755 "$tmp"
  # Rename rather than overwrite: the file being replaced may be executing.
  mv -f "$tmp" "$dest"
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

# ── the release path ─────────────────────────────────────────────────────────

# The newest release's tag, or a refusal naming which reason there is none:
# the request failed, or the project has published nothing.
latest_tag() {
  local url
  url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$REPO_URL/releases/latest")" ||
    die "could not ask $REPO_URL for its latest release — the output above says why"
  case "$url" in
    */releases/tag/*) printf '%s\n' "${url##*/}" ;;
    *) die "$REPO_URL has published no release yet — pass --build to build one" ;;
  esac
}

# Checked before the archive is fetched, so the refusal to install unverified
# bytes is a precondition rather than a decision made after the download.
require_sha256() {
  have sha256sum || have shasum ||
    die "neither sha256sum nor shasum is installed — a download cannot be verified"
}

sha256_of() {
  if have sha256sum; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

download_release() {
  local tag="$1" target="$2" dir="$3"
  local base="$REPO_URL/releases/download/$tag"
  local archive="$BINARY-$target.tar.gz" sums="$BINARY-$target.sha256"

  step "downloading $archive from $tag"
  curl -fsSL "$base/$archive" -o "$dir/$archive" ||
    die "could not download $archive from $tag — the output above says why; see $REPO_URL/releases, and if $tag is listed without this archive, run --build --rev $tag in place of --version"
  curl -fsSL "$base/$sums" -o "$dir/$sums" ||
    die "$archive downloaded but $sums did not — refusing to install bytes it cannot verify"

  local want got
  want="$(awk -v f="$archive" '$2 == f || $2 == "*"f {print $1; exit}' "$dir/$sums")"
  [ -n "$want" ] || die "$sums names no checksum for $archive"
  got="$(sha256_of "$dir/$archive")"
  [ "$want" = "$got" ] || die "checksum mismatch for $archive: expected $want, got $got"
  log "${DIM}sha256 $got${RESET}"

  tar -xzf "$dir/$archive" -C "$dir" "$BINARY" ||
    die "$archive does not contain $BINARY"
}

# ── the source path ──────────────────────────────────────────────────────────

# The version the clone's workspace declares, so this script never carries a
# second copy of it to drift from.
required_rust() {
  sed -n 's/^rust-version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$ROOT/Cargo.toml" | head -1
}

# Numeric comparison of major.minor.patch, so 1.100 sorts above 1.97 rather
# than below it. A pre-release suffix is dropped, as cargo drops it when it
# holds a compiler to `rust-version`.
version_at_least() {
  local have want i
  IFS=. read -r -a have <<<"${1%%-*}"
  IFS=. read -r -a want <<<"${2%%-*}"
  for i in 0 1 2; do
    [ "${have[i]:-0}" -gt "${want[i]:-0}" ] && return 0
    [ "${have[i]:-0}" -lt "${want[i]:-0}" ] && return 1
  done
  return 0
}

build_from_source() {
  local dir="$1"
  have cargo || die "cargo is not installed — see https://rustup.rs, then re-run this"

  # Cargo runs from the clone when there is one, so rustup selects the
  # toolchain its `rust-toolchain.toml` pins, and otherwise from the work
  # directory, which no project's toolchain file claims. Only the clone's own
  # tree is checked against its `rust-version` here: a ref is a tree cargo
  # fetches, and cargo holds the compiler to that tree's own.
  local here="${ROOT:-$dir}" from_clone=0
  if [ -n "$ROOT" ] && [ -z "$REV" ]; then
    from_clone=1
  fi

  local cargo_version want
  cargo_version="$(cd "$here" && cargo --version)" ||
    die "cargo does not run in $here — the output above says why"
  cargo_version="${cargo_version#cargo }"
  cargo_version="${cargo_version%% *}"
  if [ "$from_clone" -eq 1 ]; then
    want="$(required_rust)"
    if [ -n "$want" ] && ! version_at_least "$cargo_version" "$want"; then
      die "cargo $cargo_version is older than the Rust $want this workspace declares — install $want or newer"
    fi
  fi
  log "${DIM}cargo $cargo_version${RESET}"

  local install=(cargo install --locked --force --root "$dir/out")
  if [ "$from_clone" -eq 1 ]; then
    step "building from $ROOT"
    install+=(--path "$ROOT/$CRATE_PATH")
  else
    step "building from $REPO_URL${REV:+ @ $REV}"
    install+=(--git "$REPO_URL")
    [ -n "$REV" ] && install+=(--rev "$REV")
    install+=("harness-cli")
  fi

  (cd "$here" && "${install[@]}") || die "cargo install failed — the output above says why"
  [ -x "$dir/out/bin/$BINARY" ] || die "cargo reported success but built no $BINARY"
}

# ── run ──────────────────────────────────────────────────────────────────────

if [ "$CHECK_ONLY" -eq 1 ]; then
  report_installed || exit 1
  exit 0
fi

if [ -z "$BIN_DIR" ]; then
  BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ "$BUILD" -eq 0 ]; then
  have curl || die "curl is not installed — install it, or build from source with --build (--rev <tag> for a release)"
  TAG="$VERSION"
  if [ -z "$TAG" ]; then
    TAG="$(latest_tag)" || exit 1
  fi
  if TARGET="$(host_target)"; then
    require_sha256
    download_release "$TAG" "$TARGET" "$WORK"
    SOURCE="$WORK/$BINARY"
  else
    # An unreleased platform builds the release it would have downloaded,
    # announced rather than silently skipped.
    warn "no release binary for $(uname -s)/$(uname -m) — building $TAG from source"
    REV="$TAG"
    build_from_source "$WORK"
    SOURCE="$WORK/out/bin/$BINARY"
  fi
else
  build_from_source "$WORK"
  SOURCE="$WORK/out/bin/$BINARY"
fi

step "installing to $BIN_DIR"
place "$SOURCE"

INSTALLED="$BIN_DIR/$BINARY"
INSTALLED_VERSION="$("$INSTALLED" --version)" || die "$INSTALLED was installed but does not run"
printf '\n%s%s%s → %s\n' "$GREEN" "$INSTALLED_VERSION" "$RESET" "$INSTALLED" >&2

if ! have "$BINARY"; then
  warn "$BIN_DIR is not on PATH — add it, or the plugin will report the oracle as missing:"
  log "    export PATH=\"$BIN_DIR:\$PATH\""
else
  RESOLVED="$(command -v "$BINARY")"
  if [ "$RESOLVED" != "$INSTALLED" ]; then
    # Two copies on PATH is a version the operator cannot explain later, so
    # both are named with what each answers. The remedy offered is PATH order,
    # never deletion: what shadows a fresh install is commonly a version
    # manager's shim, which this script does not own and which would be
    # regenerated anyway.
    #
    # Only the shadow's version is read here, behind a guard: an install that
    # does not run was refused when its version was read above.
    warn "PATH resolves $BINARY to $RESOLVED, not what was just installed:"
    log "    $RESOLVED → $("$RESOLVED" --version 2>/dev/null || echo 'does not run')"
    log "    $INSTALLED → $INSTALLED_VERSION"
    log "    put $BIN_DIR first to prefer this one:"
    log "        export PATH=\"$BIN_DIR:\$PATH\""
  fi
fi

cat >&2 <<EOF

${DIM}next${RESET}
  harnex --help                  what the oracle answers
  harnex check                   run every gate this project declares
  harnex completions zsh --raw   shell completions
  /plugin marketplace add $REPO
  /plugin install harnex@harnex   the skill and commands that call it

${DIM}to remove:${RESET} rm $INSTALLED
EOF
