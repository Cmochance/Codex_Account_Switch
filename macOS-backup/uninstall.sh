#!/usr/bin/env bash
set -euo pipefail

CODHOME="${CODEX_HOME:-$HOME/.codex}"
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="auto"
FORWARD_ARGS=()
RUNTIME_CLI="$CODHOME/account_backup/macos/codex_switch_cli"
ZSHRC="$HOME/.zshrc"
NATIVE_BEGIN_MARK="# >>> Codex Account Switch Native PATH (managed) >>>"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      MODE="${2:-}"
      shift 2
      ;;
    --mode=*)
      MODE="${1#*=}"
      shift
      ;;
    --desktop)
      MODE="desktop"
      shift
      ;;
    --legacy)
      MODE="legacy"
      shift
      ;;
    *)
      FORWARD_ARGS+=("$1")
      shift
      ;;
  esac
done

case "$MODE" in
  auto)
    if [[ -x "$RUNTIME_CLI" ]] || ([[ -f "$ZSHRC" ]] && rg -F "$NATIVE_BEGIN_MARK" "$ZSHRC" >/dev/null 2>&1); then
      exec "$PROJECT_ROOT/macOS-backup/uninstall-desktop.sh" "${FORWARD_ARGS[@]}"
    fi

    exec "$PROJECT_ROOT/macOS-backup/uninstall-legacy.sh" "${FORWARD_ARGS[@]}"
    ;;
  desktop)
    exec "$PROJECT_ROOT/macOS-backup/uninstall-desktop.sh" "${FORWARD_ARGS[@]}"
    ;;
  legacy)
    exec "$PROJECT_ROOT/macOS-backup/uninstall-legacy.sh" "${FORWARD_ARGS[@]}"
    ;;
  *)
    echo "Error: unsupported mode: $MODE" >&2
    echo "Supported modes: auto, desktop, legacy" >&2
    exit 1
    ;;
esac
