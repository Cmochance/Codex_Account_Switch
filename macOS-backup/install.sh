#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="auto"
FORWARD_ARGS=()

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
    if "$PROJECT_ROOT/macOS-backup/install-desktop.sh" --check-native-cli >/dev/null 2>&1; then
      exec "$PROJECT_ROOT/macOS-backup/install-desktop.sh" "${FORWARD_ARGS[@]}"
    fi

    echo "Native macOS desktop installer not found; falling back to the legacy shell installer."
    exec "$PROJECT_ROOT/macOS-backup/install-legacy.sh" "${FORWARD_ARGS[@]}"
    ;;
  desktop)
    exec "$PROJECT_ROOT/macOS-backup/install-desktop.sh" "${FORWARD_ARGS[@]}"
    ;;
  legacy)
    exec "$PROJECT_ROOT/macOS-backup/install-legacy.sh" "${FORWARD_ARGS[@]}"
    ;;
  *)
    echo "Error: unsupported mode: $MODE" >&2
    echo "Supported modes: auto, desktop, legacy" >&2
    exit 1
    ;;
esac
