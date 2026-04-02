#!/usr/bin/env bash
set -euo pipefail

CODHOME="${CODEX_HOME:-$HOME/.codex}"
BACKUP_ROOT="$CODHOME/account_backup"
AUTO_SAVE_ROOT="$BACKUP_ROOT/_autosave"
CURRENT_PROFILE_FILE="$BACKUP_ROOT/.current_profile"
ACTIVE_MARKER_FILE=".active_profile"
APP_NAME="Codex"
APP_PATH="/Applications/Codex.app"

usage() {
  cat <<'USAGE'
Usage:
  codex switch <profile>
  codex switch list
USAGE
}

list_profiles() {
  local d name
  for d in "$BACKUP_ROOT"/*; do
    [[ -d "$d" ]] || continue
    name="$(basename "$d")"
    [[ "$name" == "_autosave" ]] && continue
    echo "$name"
  done | LC_ALL=C sort
}

resolve_current_profile() {
  local p d name

  if [[ -f "$CURRENT_PROFILE_FILE" ]]; then
    p="$(tr -d '[:space:]' < "$CURRENT_PROFILE_FILE")"
    if [[ -n "$p" && -d "$BACKUP_ROOT/$p" ]]; then
      echo "$p"
      return
    fi
  fi

  for d in "$BACKUP_ROOT"/*; do
    [[ -d "$d" ]] || continue
    name="$(basename "$d")"
    [[ "$name" == "_autosave" ]] && continue
    if [[ -f "$d/$ACTIVE_MARKER_FILE" ]]; then
      echo "$name"
      return
    fi
  done

  echo ""
}

# Save current ~/.codex managed files back to the previously active profile folder.
backup_root_state_to_profile() {
  local profile="$1"
  local profile_dir="$BACKUP_ROOT/$profile"
  local entry name src dst
  local managed_names=("auth.json")
  local dedup="::auth.json::"

  [[ -d "$profile_dir" ]] || return 0

  for entry in "$profile_dir"/*; do
    [[ -e "$entry" ]] || continue
    name="$(basename "$entry")"
    [[ "$name" == ".DS_Store" || "$name" == "$ACTIVE_MARKER_FILE" ]] && continue
    if [[ "$dedup" != *"::$name::"* ]]; then
      managed_names+=("$name")
      dedup+="${name}::"
    fi
  done

  for name in "${managed_names[@]}"; do
    src="$CODHOME/$name"
    dst="$profile_dir/$name"

    if [[ -d "$src" ]]; then
      mkdir -p "$dst"
      if command -v rsync >/dev/null 2>&1; then
        rsync -a --delete "$src/" "$dst/"
      else
        rm -rf "$dst"
        cp -R "$src" "$dst"
      fi
    elif [[ -f "$src" ]]; then
      cp "$src" "$dst"
    else
      rm -rf "$dst"
    fi
  done
}

set_active_marker() {
  local profile="$1"
  local d name

  for d in "$BACKUP_ROOT"/*; do
    [[ -d "$d" ]] || continue
    name="$(basename "$d")"
    [[ "$name" == "_autosave" ]] && continue
    rm -f "$d/$ACTIVE_MARKER_FILE"
  done

  printf 'activated_at=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$BACKUP_ROOT/$profile/$ACTIVE_MARKER_FILE"
  echo "$profile" > "$CURRENT_PROFILE_FILE"
}

is_codex_app_running() {
  pgrep -x "$APP_NAME" >/dev/null 2>&1
}

quit_codex_app_if_running() {
  local attempt

  if ! is_codex_app_running; then
    return 1
  fi

  # Use process signals to avoid the app's interactive quit confirmation.
  pkill -TERM -x "$APP_NAME" >/dev/null 2>&1 || true

  for attempt in $(seq 1 20); do
    if ! is_codex_app_running; then
      return 0
    fi
    sleep 0.2
  done

  pkill -KILL -x "$APP_NAME" >/dev/null 2>&1 || true

  for attempt in $(seq 1 10); do
    if ! is_codex_app_running; then
      return 0
    fi
    sleep 0.2
  done

  echo "Error: $APP_NAME did not exit cleanly. Close it manually and retry." >&2
  exit 1
}

reopen_codex_app_if_needed() {
  local app_was_running="$1"

  if [[ "$app_was_running" -eq 1 ]]; then
    open -a "$APP_PATH" >/dev/null 2>&1 || open -a "$APP_NAME" >/dev/null 2>&1
  fi
}

if [[ ! -d "$BACKUP_ROOT" ]]; then
  echo "Error: backup folder not found: $BACKUP_ROOT" >&2
  exit 1
fi

cmd="${1:-}"

if [[ -z "$cmd" ]]; then
  usage
  exit 1
fi

if [[ "$cmd" == "list" || "$cmd" == "--list" || "$cmd" == "-l" ]]; then
  list_profiles
  current_profile="$(resolve_current_profile)"
  if [[ -n "$current_profile" ]]; then
    echo "current: $current_profile"
  fi
  exit 0
fi

profile="$cmd"
profile_dir="$BACKUP_ROOT/$profile"

if [[ ! -d "$profile_dir" ]]; then
  echo "Error: profile not found: $profile" >&2
  echo "Available profiles:" >&2
  list_profiles >&2
  exit 1
fi

if [[ ! -f "$profile_dir/auth.json" ]]; then
  echo "Error: missing auth file: $profile_dir/auth.json" >&2
  exit 1
fi

app_was_running=0
if is_codex_app_running; then
  app_was_running=1
  quit_codex_app_if_running
fi

current_profile="$(resolve_current_profile)"
if [[ -n "$current_profile" ]]; then
  backup_root_state_to_profile "$current_profile"
fi

mkdir -p "$AUTO_SAVE_ROOT"
if [[ -f "$CODHOME/auth.json" ]]; then
  ts="$(date +%Y%m%d-%H%M%S)"
  mkdir -p "$AUTO_SAVE_ROOT/$ts"
  cp "$CODHOME/auth.json" "$AUTO_SAVE_ROOT/$ts/auth.json"
fi

if command -v rsync >/dev/null 2>&1; then
  rsync -a --exclude '.DS_Store' --exclude "$ACTIVE_MARKER_FILE" "$profile_dir/" "$CODHOME/"
else
  find "$profile_dir" -mindepth 1 -maxdepth 1 -print0 | while IFS= read -r -d '' entry; do
    name="$(basename "$entry")"
    [[ "$name" == ".DS_Store" || "$name" == "$ACTIVE_MARKER_FILE" ]] && continue
    cp -R "$entry" "$CODHOME/$name"
  done
fi

set_active_marker "$profile"
reopen_codex_app_if_needed "$app_was_running"

echo "Switched to profile: $profile"
if [[ -n "$current_profile" ]]; then
  echo "Backed up current root state to profile: $current_profile"
fi
echo "Auth file replaced: $CODHOME/auth.json"
