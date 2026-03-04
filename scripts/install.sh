#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_SCRIPT="$PROJECT_ROOT/scripts/codex-switch.sh"
CODHOME="${CODEX_HOME:-$HOME/.codex}"
TARGET_DIR="$CODHOME/account_backup"
TARGET_SCRIPT="$TARGET_DIR/codex-switch.sh"
ZSHRC="$HOME/.zshrc"
BEGIN_MARK="# >>> Codex Account Switch (managed) >>>"
END_MARK="# <<< Codex Account Switch (managed) <<<"
NO_SHELL=0

if [[ "${1:-}" == "--no-shell" ]]; then
  NO_SHELL=1
fi

if [[ ! -f "$SOURCE_SCRIPT" ]]; then
  echo "Error: source script not found: $SOURCE_SCRIPT" >&2
  exit 1
fi

mkdir -p "$TARGET_DIR"
cp "$SOURCE_SCRIPT" "$TARGET_SCRIPT"
chmod +x "$TARGET_SCRIPT"

echo "Installed switch script to: $TARGET_SCRIPT"

if [[ "$NO_SHELL" -eq 1 ]]; then
  echo "Skipped shell wrapper injection (--no-shell)."
  exit 0
fi

if [[ ! -f "$ZSHRC" ]]; then
  touch "$ZSHRC"
fi

if rg -F "$BEGIN_MARK" "$ZSHRC" >/dev/null 2>&1; then
  tmp_file="$(mktemp)"
  awk -v begin="$BEGIN_MARK" -v end="$END_MARK" '
    BEGIN { skip = 0 }
    $0 == begin { skip = 1; next }
    $0 == end { skip = 0; next }
    !skip { print }
  ' "$ZSHRC" > "$tmp_file"
  mv "$tmp_file" "$ZSHRC"
fi

cat >> "$ZSHRC" <<'WRAPPER'

# >>> Codex Account Switch (managed) >>>
codex() {
  if [[ "${1:-}" == "switch" ]]; then
    shift
    "${CODEX_HOME:-$HOME/.codex}/account_backup/codex-switch.sh" "$@"
    return $?
  fi
  command codex "$@"
}
# <<< Codex Account Switch (managed) <<<
WRAPPER

echo "Updated shell wrapper in: $ZSHRC"
echo "Run: source ~/.zshrc"
