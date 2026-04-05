#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_SCRIPT="$PROJECT_ROOT/macOS/codex-switch.sh"
PLACEHOLDER_AUTH_TEMPLATE="$PROJECT_ROOT/examples/account_backup/demo/auth.json.example"
CODHOME="${CODEX_HOME:-$HOME/.codex}"
TARGET_DIR="$CODHOME/account_backup"
TARGET_SCRIPT="$TARGET_DIR/codex-switch.sh"
DEFAULT_PROFILES=(a b c d)
DEFAULT_PROFILE_DIR="$TARGET_DIR/a"
ROOT_AUTH_FILE="$CODHOME/auth.json"
DEFAULT_PROFILE_AUTH_FILE="$DEFAULT_PROFILE_DIR/auth.json"
CURRENT_PROFILE_FILE="$TARGET_DIR/.current_profile"
ACTIVE_MARKER_FILE=".active_profile"
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

if [[ ! -f "$PLACEHOLDER_AUTH_TEMPLATE" ]]; then
  echo "Error: placeholder auth template not found: $PLACEHOLDER_AUTH_TEMPLATE" >&2
  exit 1
fi

has_initialized_active_profile() {
  local profile d

  if [[ -f "$CURRENT_PROFILE_FILE" ]]; then
    profile="$(tr -d '[:space:]' < "$CURRENT_PROFILE_FILE")"
    if [[ -n "$profile" && -d "$TARGET_DIR/$profile" ]]; then
      return 0
    fi
  fi

  for d in "$TARGET_DIR"/*; do
    [[ -d "$d" ]] || continue
    if [[ -f "$d/$ACTIVE_MARKER_FILE" ]]; then
      return 0
    fi
  done

  return 1
}

initialize_default_active_profile() {
  printf 'a\n' > "$CURRENT_PROFILE_FILE"
  printf 'activated_at=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$DEFAULT_PROFILE_DIR/$ACTIVE_MARKER_FILE"
}

mkdir -p "$TARGET_DIR"
cp "$SOURCE_SCRIPT" "$TARGET_SCRIPT"
chmod +x "$TARGET_SCRIPT"

placeholder_created=()
for profile in "${DEFAULT_PROFILES[@]}"; do
  mkdir -p "$TARGET_DIR/$profile"
  auth_file="$TARGET_DIR/$profile/auth.json"
  if [[ ! -f "$auth_file" ]]; then
    cp "$PLACEHOLDER_AUTH_TEMPLATE" "$auth_file"
    chmod 600 "$auth_file"
    placeholder_created+=("$auth_file")
  fi
done

if [[ -f "$ROOT_AUTH_FILE" ]]; then
  cp "$ROOT_AUTH_FILE" "$DEFAULT_PROFILE_AUTH_FILE"
  chmod 600 "$DEFAULT_PROFILE_AUTH_FILE"
  echo "Backed up current login to: $DEFAULT_PROFILE_AUTH_FILE"
  if ! has_initialized_active_profile; then
    initialize_default_active_profile
    echo "Initialized default active profile: a"
  fi
else
  echo "Warning: current auth.json not found at $ROOT_AUTH_FILE; left profile auth files as placeholders." >&2
fi

if [[ "${#placeholder_created[@]}" -gt 0 ]]; then
  echo "Created placeholder auth templates:"
  for auth_file in "${placeholder_created[@]}"; do
    echo "- $auth_file"
  done
fi

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
