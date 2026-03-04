#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="$PROJECT_ROOT/scripts/codex-switch.sh"

if [[ ! -x "$SCRIPT" ]]; then
  echo "Missing executable: $SCRIPT" >&2
  exit 1
fi

TEST_HOME="$(mktemp -d /tmp/codex-switch-smoke.XXXXXX)"
cleanup() {
  rm -rf "$TEST_HOME"
}
trap cleanup EXIT

mkdir -p "$TEST_HOME/account_backup/a" "$TEST_HOME/account_backup/b"
printf '{"token":"A-OLD"}\n' > "$TEST_HOME/account_backup/a/auth.json"
printf '{"token":"B-OLD"}\n' > "$TEST_HOME/account_backup/b/auth.json"
printf '{"token":"A-LIVE"}\n' > "$TEST_HOME/auth.json"
printf 'a\n' > "$TEST_HOME/account_backup/.current_profile"
printf 'activated_at=2026-03-04T00:00:00Z\n' > "$TEST_HOME/account_backup/a/.active_profile"

CODEX_HOME="$TEST_HOME" "$SCRIPT" b >/tmp/codex-switch-smoke.out

root_auth="$(cat "$TEST_HOME/auth.json")"
auth_a="$(cat "$TEST_HOME/account_backup/a/auth.json")"
current_profile="$(cat "$TEST_HOME/account_backup/.current_profile")"

if [[ "$root_auth" != '{"token":"B-OLD"}' ]]; then
  echo "FAIL: root auth not switched to profile b"
  exit 1
fi

if [[ "$auth_a" != '{"token":"A-LIVE"}' ]]; then
  echo "FAIL: profile a was not updated with previous root auth"
  exit 1
fi

if [[ "$current_profile" != 'b' ]]; then
  echo "FAIL: current profile marker not updated"
  exit 1
fi

if [[ ! -f "$TEST_HOME/account_backup/b/.active_profile" ]]; then
  echo "FAIL: active marker missing in target profile"
  exit 1
fi

echo "PASS: smoke test completed"
