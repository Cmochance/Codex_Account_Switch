#!/usr/bin/env bash
set -euo pipefail

CODHOME="${CODEX_HOME:-$HOME/.codex}"
TARGET_SCRIPT="$CODHOME/account_backup/codex-switch.sh"
ZSHRC="$HOME/.zshrc"
BEGIN_MARK="# >>> Codex Account Switch (managed) >>>"
END_MARK="# <<< Codex Account Switch (managed) <<<"
REMOVE_SCRIPT=0

if [[ "${1:-}" == "--remove-script" ]]; then
  REMOVE_SCRIPT=1
fi

if [[ -f "$ZSHRC" ]] && rg -F "$BEGIN_MARK" "$ZSHRC" >/dev/null 2>&1; then
  tmp_file="$(mktemp)"
  awk -v begin="$BEGIN_MARK" -v end="$END_MARK" '
    BEGIN { skip = 0 }
    $0 == begin { skip = 1; next }
    $0 == end { skip = 0; next }
    !skip { print }
  ' "$ZSHRC" > "$tmp_file"
  mv "$tmp_file" "$ZSHRC"
  echo "Removed managed wrapper block from: $ZSHRC"
else
  echo "No managed wrapper block found in: $ZSHRC"
fi

if [[ "$REMOVE_SCRIPT" -eq 1 ]]; then
  rm -f "$TARGET_SCRIPT"
  echo "Removed script: $TARGET_SCRIPT"
else
  echo "Switch script kept at: $TARGET_SCRIPT"
fi

echo "Run: source ~/.zshrc"
