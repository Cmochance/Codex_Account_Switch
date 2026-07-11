#!/usr/bin/env bash
set -euo pipefail

CODHOME="${CODEX_HOME:-$HOME/.codex}"
BACKUP_ROOT="$CODHOME/account_backup"
AUTO_SAVE_ROOT="$BACKUP_ROOT/_autosave"
CURRENT_PROFILE_FILE="$BACKUP_ROOT/.current_profile"
ACTIVE_MARKER_FILE=".active_profile"
# OpenAI folded Codex into ChatGPT.app (bundle id still com.openai.codex).
# Keep the legacy Codex.app path as a fallback for older installs. A
# candidate only qualifies if it embeds the codex CLI: a directory check
# alone would match a chat-only ChatGPT.app (bundle id com.openai.chat)
# and we would open the consumer chat client instead of the codex host.
APP_BUNDLE_ID="com.openai.codex"
APP_NAME_CHATGPT="ChatGPT"
APP_NAME_CODEX="Codex"
APP_PATH=""
for app_candidate in \
  "/Applications/ChatGPT.app" \
  "$HOME/Applications/ChatGPT.app" \
  "/Applications/Codex.app" \
  "$HOME/Applications/Codex.app"; do
  if [[ -f "$app_candidate/Contents/Resources/codex" ]]; then
    APP_PATH="$app_candidate"
    break
  fi
done
unset app_candidate

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

# Echo the PIDs of processes that both match one of the known host
# process names AND live inside an app bundle whose CFBundleIdentifier
# is com.openai.codex. The name match alone is not enough: the consumer
# ChatGPT chat client (com.openai.chat) also ships an executable named
# "ChatGPT", and matching by bare name would count — and later kill — it
# as collateral. Unverifiable PIDs are skipped (fails safe: the quit
# wait loop times out instead of signalling an unidentified process).
codex_desktop_pids() {
  local name pid exe bundle_root bundle_id
  for name in "$APP_NAME_CHATGPT" "$APP_NAME_CODEX"; do
    for pid in $(pgrep -x "$name" 2>/dev/null || true); do
      # macOS `ps -o comm=` prints the full executable path.
      exe="$(ps -o comm= -p "$pid" 2>/dev/null || true)"
      [[ "$exe" == *.app/* ]] || continue
      bundle_root="${exe%.app/*}.app"
      bundle_id="$(defaults read "$bundle_root/Contents/Info" CFBundleIdentifier 2>/dev/null || true)"
      if [[ "$bundle_id" == "$APP_BUNDLE_ID" ]]; then
        echo "$pid"
      fi
    done
  done
  return 0
}

signal_codex_desktop() {
  local signal="$1" pid
  for pid in $(codex_desktop_pids); do
    kill "$signal" "$pid" >/dev/null 2>&1 || true
  done
}

is_codex_app_running() {
  # Bundle-id check is rename-proof, and a definitive "false" from
  # osascript is trusted as-is: com.openai.codex is shared by every
  # known host bundle, so "not running" is authoritative and probing
  # process names after it would only add false positives (a bare name
  # match could be the consumer ChatGPT chat client). Identity-verified
  # process probes are consulted solely when osascript itself is
  # unavailable. Same semantics as the Rust implementation in
  # src-tauri/mac/runtime/process.rs.
  local answer
  answer="$(osascript -e "application id \"$APP_BUNDLE_ID\" is running" 2>/dev/null | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]' || true)"
  case "$answer" in
    true) return 0 ;;
    false) return 1 ;;
  esac
  [[ -n "$(codex_desktop_pids)" ]]
}

quit_codex_app_if_running() {
  local attempt

  if ! is_codex_app_running; then
    return 1
  fi

  # Polite AppleScript quit lets the host flush state, but must NOT be
  # waited on: the host can pop an interactive quit-confirmation dialog,
  # and the first Apple event may hang on the macOS Automation (TCC)
  # consent prompt — either would block the switch for minutes. Run it
  # in the background and TERM immediately (by verified PID), as the
  # pre-merge script did; the wait loop below provides the grace period.
  osascript -e "tell application id \"$APP_BUNDLE_ID\" to quit" >/dev/null 2>&1 &
  signal_codex_desktop -TERM

  for attempt in $(seq 1 20); do
    if ! is_codex_app_running; then
      return 0
    fi
    sleep 0.2
  done

  signal_codex_desktop -KILL

  for attempt in $(seq 1 10); do
    if ! is_codex_app_running; then
      return 0
    fi
    sleep 0.2
  done

  echo "Error: Codex/ChatGPT did not exit cleanly. Close it manually and retry." >&2
  exit 1
}

reopen_codex_app_if_needed() {
  local app_was_running="$1"

  if [[ "$app_was_running" -eq 1 ]]; then
    if [[ -n "$APP_PATH" ]]; then
      open -a "$APP_PATH" >/dev/null 2>&1 && return 0
    fi
    # The trailing warning also keeps the chain's overall status zero —
    # under `set -e` a fully failed open chain would otherwise abort the
    # script here, AFTER the switch already happened but BEFORE the
    # success output, making a completed switch look like a failure.
    open -b "$APP_BUNDLE_ID" >/dev/null 2>&1 \
      || open -a "$APP_NAME_CHATGPT" >/dev/null 2>&1 \
      || open -a "$APP_NAME_CODEX" >/dev/null 2>&1 \
      || echo "Warning: profile switch completed, but Codex/ChatGPT could not be relaunched — open it manually." >&2
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
