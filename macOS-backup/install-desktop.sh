#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CODHOME="${CODEX_HOME:-$HOME/.codex}"
BIN_DIR="$CODHOME/bin"
RUNTIME_CLI="$CODHOME/account_backup/macos/codex_switch_cli"
ZSHRC="$HOME/.zshrc"
NATIVE_BEGIN_MARK="# >>> Codex Account Switch Native PATH (managed) >>>"
NATIVE_END_MARK="# <<< Codex Account Switch Native PATH (managed) <<<"
LEGACY_BEGIN_MARK="# >>> Codex Account Switch (managed) >>>"
LEGACY_END_MARK="# <<< Codex Account Switch (managed) <<<"
CHECK_NATIVE_CLI=0
NO_SHELL=0
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

  while IFS= read -r candidate; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done < <(
    find "$PROJECT_ROOT/src-tauri/target" -type f -name codex_switch \
      \( -path "*/bundle/macos/*.app/Contents/MacOS/codex_switch" -o -path "*/release/codex_switch" -o -path "*/debug/codex_switch" \) \
      2>/dev/null
  )

  return 1
}

ensure_native_shell_hook() {
  mkdir -p "$BIN_DIR"

  if [[ ! -f "$ZSHRC" ]]; then
    touch "$ZSHRC"
  fi

  remove_managed_block "$ZSHRC" "$NATIVE_BEGIN_MARK" "$NATIVE_END_MARK"
  remove_managed_block "$ZSHRC" "$LEGACY_BEGIN_MARK" "$LEGACY_END_MARK"

  cat >> "$ZSHRC" <<'HOOK'

# >>> Codex Account Switch Native PATH (managed) >>>
export PATH="${CODEX_HOME:-$HOME/.codex}/bin:$PATH"
# <<< Codex Account Switch Native PATH (managed) <<<
HOOK
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --check-native-cli)
      CHECK_NATIVE_CLI=1
      shift
      ;;
    --no-shell)
      NO_SHELL=1
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

native_cli="$(resolve_native_cli)" || {
  echo "Error: native macOS installer binary not found. Build the desktop runtime first or set CODEX_SWITCH_NATIVE_CLI." >&2
  exit 1
}

if [[ "$CHECK_NATIVE_CLI" -eq 1 ]]; then
  printf '%s\n' "$native_cli"
  exit 0
fi

CODEX_HOME="$CODHOME" "$native_cli" install

if [[ "$NO_SHELL" -eq 1 ]]; then
  echo "Skipped shell PATH injection (--no-shell)."
  exit 0
fi

ensure_native_shell_hook
echo "Updated shell PATH hook in: $ZSHRC"
echo "Run: source ~/.zshrc"
