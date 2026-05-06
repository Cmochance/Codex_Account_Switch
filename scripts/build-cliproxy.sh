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

if [[ ! -d "${SOURCE_DIR}" ]]; then
  echo "[build-cliproxy] Source not found at ${SOURCE_DIR}." >&2
  echo "                 Re-clone with: git clone --depth 1 https://github.com/router-for-me/CLIProxyAPI.git docs/CLIProxyAPI" >&2
  exit 1
fi

mkdir -p "${OUTPUT_DIR}"

resolve_triple() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo "aarch64-apple-darwin" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin" ;;
    Linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
    Linux-aarch64) echo "aarch64-unknown-linux-gnu" ;;
    *) echo "" ;;
  esac
}

resolve_goarch() {
  case "$(uname -m)" in
    arm64|aarch64) echo "arm64" ;;
    x86_64) echo "amd64" ;;
    *) echo "" ;;
  esac
}

resolve_goos() {
  case "$(uname -s)" in
    Darwin) echo "darwin" ;;
    Linux) echo "linux" ;;
    *) echo "" ;;
  esac
}

TRIPLE_OVERRIDE="${1:-}"
TRIPLE="${TRIPLE_OVERRIDE:-$(resolve_triple)}"
GOARCH_VALUE="$(resolve_goarch)"
GOOS_VALUE="$(resolve_goos)"

if [[ -z "${TRIPLE}" || -z "${GOARCH_VALUE}" || -z "${GOOS_VALUE}" ]]; then
  echo "[build-cliproxy] Unsupported host. Pass a Rust target triple as the first argument." >&2
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
