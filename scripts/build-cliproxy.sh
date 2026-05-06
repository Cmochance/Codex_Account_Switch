#!/usr/bin/env bash
# Build the CLIProxyAPI sidecar from docs/CLIProxyAPI and place the binary
# under src-tauri/binaries/ using the Tauri externalBin naming convention
# (`<base>-<rust-target-triple><ext>`). The bundler resolves these names at
# package time; at runtime the suffix is stripped and the binary lands beside
# the main executable.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_DIR="${ROOT_DIR}/docs/CLIProxyAPI"
OUTPUT_DIR="${ROOT_DIR}/src-tauri/binaries"
BASE_NAME="cliproxy"

if ! command -v go >/dev/null 2>&1; then
  echo "[build-cliproxy] Go toolchain not found. Install from https://go.dev/dl/." >&2
  exit 1
fi

# CLIProxyAPI lives outside this repo (docs/ is gitignored). Clone it on
# first run so a fresh checkout — including CI runners — can build the
# sidecar without manual prep.
CLIPROXY_REPO_URL="${CLIPROXY_REPO_URL:-https://github.com/router-for-me/CLIProxyAPI.git}"
if [[ ! -d "${SOURCE_DIR}" ]]; then
  if ! command -v git >/dev/null 2>&1; then
    echo "[build-cliproxy] git not available; cannot clone ${CLIPROXY_REPO_URL}." >&2
    exit 1
  fi
  echo "[build-cliproxy] Source missing at ${SOURCE_DIR}; cloning from ${CLIPROXY_REPO_URL}." >&2
  mkdir -p "$(dirname "${SOURCE_DIR}")"
  git clone --depth 1 "${CLIPROXY_REPO_URL}" "${SOURCE_DIR}"
fi

mkdir -p "${OUTPUT_DIR}"

resolve_host_triple() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo "aarch64-apple-darwin" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin" ;;
    Linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
    Linux-aarch64) echo "aarch64-unknown-linux-gnu" ;;
    *) echo "" ;;
  esac
}

# Derive Go's GOOS / GOARCH from the requested Rust target triple so that
# `build-cliproxy.sh x86_64-apple-darwin` produced on an arm64 host still
# cross-compiles to an Intel binary instead of relabeling a host build.
goos_for_triple() {
  case "$1" in
    *darwin*)  echo "darwin" ;;
    *linux*)   echo "linux" ;;
    *windows*) echo "windows" ;;
    *) echo "" ;;
  esac
}

goarch_for_triple() {
  case "$1" in
    aarch64*|arm64*) echo "arm64" ;;
    x86_64*|amd64*)  echo "amd64" ;;
    *) echo "" ;;
  esac
}

TRIPLE_OVERRIDE="${1:-}"
TRIPLE="${TRIPLE_OVERRIDE:-$(resolve_host_triple)}"

if [[ -z "${TRIPLE}" ]]; then
  echo "[build-cliproxy] Unsupported host. Pass a Rust target triple as the first argument." >&2
  exit 1
fi

GOOS_VALUE="$(goos_for_triple "${TRIPLE}")"
GOARCH_VALUE="$(goarch_for_triple "${TRIPLE}")"

if [[ -z "${GOOS_VALUE}" || -z "${GOARCH_VALUE}" ]]; then
  echo "[build-cliproxy] Could not derive GOOS/GOARCH from triple '${TRIPLE}'." >&2
  exit 1
fi

EXT=""
case "${TRIPLE}" in
  *windows*) EXT=".exe" ;;
esac

OUTPUT_PATH="${OUTPUT_DIR}/${BASE_NAME}-${TRIPLE}${EXT}"

echo "[build-cliproxy] target=${TRIPLE}  go=${GOOS_VALUE}/${GOARCH_VALUE}"
echo "[build-cliproxy] output=${OUTPUT_PATH}"

(
  cd "${SOURCE_DIR}"
  CGO_ENABLED=0 GOOS="${GOOS_VALUE}" GOARCH="${GOARCH_VALUE}" \
    go build -trimpath -ldflags="-s -w" -o "${OUTPUT_PATH}" ./cmd/server
)

echo "[build-cliproxy] done."
