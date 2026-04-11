#!/bin/sh
# install.sh — Install azure-pipelines-cli from GitHub Releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/ljtill/azure-pipelines-cli/main/install.sh | sh
#
# Environment variables:
#   VERSION   — Pin to a specific version (e.g., "0.2.0"). Defaults to latest.
#   INSTALL_DIR — Override install directory. Defaults to ~/.local/bin.

set -eu

REPO="ljtill/azure-pipelines-cli"
BINARY_NAME="pipelines"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# --- helpers ----------------------------------------------------------------

die() { echo "Error: $*" >&2; exit 1; }

has() { command -v "$1" >/dev/null 2>&1; }

download() {
  url="$1"; dest="$2"
  if has curl; then
    curl -fsSL "$url" -o "$dest"
  elif has wget; then
    wget -qO "$dest" "$url"
  else
    die "curl or wget is required"
  fi
}

download_text() {
  url="$1"
  if has curl; then
    curl -fsSL "$url"
  elif has wget; then
    wget -qO- "$url"
  else
    die "curl or wget is required"
  fi
}

compute_sha256() {
  file="$1"
  if has sha256sum; then
    sha256sum "$file" | awk '{print $1}'
  elif has shasum; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    die "sha256sum or shasum is required to verify downloads"
  fi
}

# --- detect platform --------------------------------------------------------

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  os="linux" ;;
  Darwin) os="darwin" ;;
  *)      die "Unsupported operating system: $OS" ;;
esac

case "$ARCH" in
  x86_64|amd64)       arch="amd64" ;;
  aarch64|arm64)      arch="arm64" ;;
  *)                  die "Unsupported architecture: $ARCH" ;;
esac

ARTIFACT="${BINARY_NAME}-${os}-${arch}"

# --- resolve version --------------------------------------------------------

if [ -z "${VERSION:-}" ]; then
  echo "Fetching latest release..."
  AUTH_HEADER=""
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    AUTH_HEADER="Authorization: token ${GITHUB_TOKEN}"
  fi
  if has curl; then
    VERSION="$(curl -fsSL ${AUTH_HEADER:+-H "$AUTH_HEADER"} "https://api.github.com/repos/${REPO}/releases/latest" \
      | grep '"tag_name"' | sed -E 's/.*"v([^"]+)".*/\1/')"
  elif has wget; then
    VERSION="$(wget -qO- ${AUTH_HEADER:+--header="$AUTH_HEADER"} "https://api.github.com/repos/${REPO}/releases/latest" \
      | grep '"tag_name"' | sed -E 's/.*"v([^"]+)".*/\1/')"
  fi
  [ -z "$VERSION" ] && die "Could not determine latest version"
fi

TAG="v${VERSION}"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ARTIFACT}"
CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${TAG}/SHA256SUMS"

# --- download and install ---------------------------------------------------

echo "Installing ${BINARY_NAME} ${TAG} (${os}/${arch})..."

mkdir -p "$INSTALL_DIR"

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

download "$URL" "$TMP"
CHECKSUMS="$(download_text "$CHECKSUMS_URL")"
EXPECTED="$(printf '%s\n' "$CHECKSUMS" | awk -v artifact="$ARTIFACT" '$2 == artifact { print $1; exit }')"
[ -n "$EXPECTED" ] || die "Could not find checksum for ${ARTIFACT}"
ACTUAL="$(compute_sha256 "$TMP")"
[ "$ACTUAL" = "$EXPECTED" ] || die "Checksum mismatch for ${ARTIFACT}"
chmod +x "$TMP"
mv "$TMP" "${INSTALL_DIR}/${BINARY_NAME}"

echo "Installed to ${INSTALL_DIR}/${BINARY_NAME}"

# --- PATH check -------------------------------------------------------------

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo ""
    echo "Add ${INSTALL_DIR} to your PATH:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    echo ""
    echo "To make it permanent, add the line above to your ~/.bashrc, ~/.zshrc, or equivalent."
    ;;
esac
