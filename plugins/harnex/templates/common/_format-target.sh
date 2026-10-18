#!/usr/bin/env bash
# Resolve one regular, non-symlinked file inside the physical project root.
# The hook runner has entered that root; resolution sets FILE and FORMAT_ROOT.
resolve_format_target() {
  local candidate root parent
  [[ -n "${1:-}" && -f "$1" && ! -L "$1" ]] || return 1
  # The external pwd resolves directory casing; the sentinel preserves newlines.
  root=$(CDPATH='' cd -P -- . && env pwd -P && printf '.') || return 1
  root=${root%$'\n.'}
  root=${root%/}/
  candidate=$1
  [[ "$candidate" == /* ]] || candidate="${root}${candidate}"
  parent=${candidate%/*}
  parent=$(CDPATH='' cd -P -- "${parent:-/}" && env pwd -P && printf '.') || return 1
  parent=${parent%$'\n.'}
  candidate="${parent%/}/${candidate##*/}"
  [[ "$candidate" == "$root"* ]] || return 1
  FORMAT_ROOT=${root%/}
  [[ -n "$FORMAT_ROOT" ]] || FORMAT_ROOT=/
  FILE=$candidate
}
