#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="$PROJECT_ROOT/scripts/codex-switch.sh"

if [[ ! -x "$SCRIPT" ]]; then
  echo "Missing executable: $SCRIPT" >&2
  exit 1
fi

TEST_ROOT="$(mktemp -d /tmp/codex-switch-smoke.XXXXXX)"
cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

assert_eq() {
  local actual="$1"
  local expected="$2"
  local msg="$3"
  if [[ "$actual" != "$expected" ]]; then
    echo "FAIL: $msg"
    echo "  expected: $expected"
    echo "  actual:   $actual"
    exit 1
  fi
}

assert_file_exists() {
  local path="$1"
  local msg="$2"
  if [[ ! -e "$path" ]]; then
    echo "FAIL: $msg"
    exit 1
  fi
}

assert_file_missing() {
  local path="$1"
  local msg="$2"
  if [[ -e "$path" ]]; then
    echo "FAIL: $msg"
    exit 1
  fi
}

run_case_standard_switch_remove_copy() {
  local test_home="$TEST_ROOT/case-standard"
  mkdir -p "$test_home/account_backup/a" "$test_home/account_backup/b"
  printf '{"token":"A-OLD"}\n' > "$test_home/account_backup/a/auth.json"
  printf '{"token":"B-OLD"}\n' > "$test_home/account_backup/b/auth.json"
  printf '{"state":"A-OLD"}\n' > "$test_home/account_backup/a/state.json"
  printf '{"token":"A-LIVE"}\n' > "$test_home/auth.json"
  printf '{"state":"A-LIVE"}\n' > "$test_home/state.json"
  printf 'a\n' > "$test_home/account_backup/.current_profile"
  printf 'activated_at=2026-03-04T00:00:00Z\n' > "$test_home/account_backup/a/.active_profile"

  CODEX_HOME="$test_home" "$SCRIPT" b >/tmp/codex-switch-smoke-standard.out

  assert_eq "$(cat "$test_home/auth.json")" '{"token":"B-OLD"}' "root auth not switched to profile b"
  assert_eq "$(cat "$test_home/account_backup/a/auth.json")" '{"token":"A-LIVE"}' "profile a auth not updated from current root"
  assert_eq "$(cat "$test_home/account_backup/a/state.json")" '{"state":"A-LIVE"}' "profile a state not updated from current root"
  assert_eq "$(cat "$test_home/account_backup/.current_profile")" 'b' "current profile marker not updated to b"
  assert_file_exists "$test_home/account_backup/b/.active_profile" "active marker missing in target profile b"
  assert_file_missing "$test_home/state.json" "root stale managed file was not removed before copy"
}

run_case_auto_create_profile_skip_copy() {
  local test_home="$TEST_ROOT/case-create"
  mkdir -p "$test_home/account_backup"
  printf '{"token":"FIRST"}\n' > "$test_home/auth.json"

  CODEX_HOME="$test_home" "$SCRIPT" c >/tmp/codex-switch-smoke-create.out

  assert_file_exists "$test_home/account_backup/c" "target profile folder c was not auto-created"
  assert_file_exists "$test_home/account_backup/c/.active_profile" "active marker missing in new profile c"
  assert_eq "$(cat "$test_home/account_backup/.current_profile")" 'c' "current profile marker not updated to c"
  assert_eq "$(cat "$test_home/account_backup/a/auth.json")" '{"token":"FIRST"}' "first-time backup was not written to default profile a"
  assert_file_missing "$test_home/auth.json" "root auth should be removed when new profile has no files to copy"
}

run_case_standard_switch_remove_copy
run_case_auto_create_profile_skip_copy

echo "PASS: smoke test completed"
