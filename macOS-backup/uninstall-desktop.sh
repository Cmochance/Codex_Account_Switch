#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CODHOME="${CODEX_HOME:-$HOME/.codex}"
RUNTIME_CLI="$CODHOME/account_backup/macos/codex_switch_cli"
ZSHRC="$HOME/.zshrc"
NATIVE_BEGIN_MARK="# >>> Codex Account Switch Native PATH (managed) >>>"
NATIVE_END_MARK="# <<< Codex Account Switch Native PATH (managed) <<<"
LEGACY_BEGIN_MARK="# >>> Codex Account Switch (managed) >>>"
LEGACY_END_MARK="# <<< Codex Account Switch (managed) <<<"
REMOVE_SCRIPT=0
NATIVE_CLI_OVERRIDE="${CODEX_SWITCH_NATIVE_CLI:-}"

remove_managed_block() {
  local file="$1"
  local begin="$2"
  local end="$3"

  if [[ ! -f "$file" ]] || ! rg -F "$begin" "$file" >/dev/null 2>&1; then
    return 0
  fi

  local tmp_file
  tmp_file="$(mktemp)"
  awk -v begin="$begin" -v end="$end" '
    BEGIN { skip = 0 }
    $0 == begin { skip = 1; next }
    $0 == end { skip = 0; next }
    !skip { print }
  ' "$file" > "$tmp_file"
  mv "$tmp_file" "$file"
}

resolve_native_cli() {
  local candidate

  if [[ -n "$NATIVE_CLI_OVERRIDE" ]]; then
    if [[ -x "$NATIVE_CLI_OVERRIDE" ]]; then
      printf '%s\n' "$NATIVE_CLI_OVERRIDE"
      return 0
    fi
    echo "Error: native CLI override is not executable: $NATIVE_CLI_OVERRIDE" >&2
    return 1
  fi

  if [[ -x "$RUNTIME_CLI" ]]; then
    printf '%s\n' "$RUNTIME_CLI"
    return 0
  fi

  for candidate in \
    "$PROJECT_ROOT/src-tauri/target/release/codex_switch" \
    "$PROJECT_ROOT/src-tauri/target/debug/codex_switch"; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  return 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --remove-script)
      REMOVE_SCRIPT=1
      shift
      ;;
    --native-cli)
      NATIVE_CLI_OVERRIDE="${2:-}"
      shift 2
      ;;
    --native-cli=*)
      NATIVE_CLI_OVERRIDE="${1#*=}"
      shift
      ;;
    *)
      echo "Error: unsupported argument: $1" >&2
      exit 1
      ;;
  esac
done

remove_managed_block "$ZSHRC" "$NATIVE_BEGIN_MARK" "$NATIVE_END_MARK"
remove_managed_block "$ZSHRC" "$LEGACY_BEGIN_MARK" "$LEGACY_END_MARK"

if [[ -f "$ZSHRC" ]]; then
  echo "Removed managed shell hooks from: $ZSHRC"
fi

if native_cli="$(resolve_native_cli)"; then
  if [[ "$REMOVE_SCRIPT" -eq 1 ]]; then
    CODEX_HOME="$CODHOME" "$native_cli" uninstall --remove-script
  else
    CODEX_HOME="$CODHOME" "$native_cli" uninstall
  fi
else
  echo "No native desktop installer binary found; only shell hooks were removed."
fi

echo "Run: source ~/.zshrc"
