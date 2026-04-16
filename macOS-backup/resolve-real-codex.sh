#!/usr/bin/env bash
set -euo pipefail

MANAGED_SHIM_PATH="${1:-}"
ZSH_BIN="${ZSH:-/bin/zsh}"

if [[ ! -x "$ZSH_BIN" ]]; then
  ZSH_BIN="/bin/zsh"
fi

"$ZSH_BIN" -lic '
managed_shim="$1"

for dir in $path; do
  candidate="$dir/codex"
  [[ -x "$candidate" ]] || continue
  if [[ -n "$managed_shim" && "$candidate" == "$managed_shim" ]]; then
    continue
  fi
  printf "%s\n" "$candidate"
  exit 0
done

exit 1
' codex-switch-resolver "$MANAGED_SHIM_PATH"
