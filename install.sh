#!/bin/sh
# install.sh — Install azure-devops-cli from GitHub Releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | sh
#
# Environment variables:
#   VERSION   — Pin to a specific version (e.g., "0.2.0"). Defaults to latest.
#   INSTALL_DIR — Override install directory. Defaults to ~/.local/bin.

set -eu

REPO="ljtill/azure-devops-cli"
BINARY_NAME="devops"
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

INNER_BINARY="${BINARY_NAME}-${os}-${arch}"
ARTIFACT="${INNER_BINARY}.tar.gz"

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
COSIGN_BUNDLE_URL="https://github.com/${REPO}/releases/download/${TAG}/SHA256SUMS.cosign.bundle"
COSIGN_CERT_IDENTITY_RE='^https://github\.com/ljtill/azure-devops-cli/\.github/workflows/ci\.release\.yml@refs/heads/main$'
COSIGN_OIDC_ISSUER='https://token.actions.githubusercontent.com'
ISSUES_URL="https://github.com/${REPO}/issues"

# --- validate platform is published

RELEASE_API_URL="https://api.github.com/repos/${REPO}/releases/tags/${TAG}"
API_AUTH_HEADER=""
if [ -n "${GITHUB_TOKEN:-}" ]; then
  API_AUTH_HEADER="Authorization: Bearer ${GITHUB_TOKEN}"
fi

RELEASE_JSON=""
if has curl; then
  RELEASE_JSON="$(curl -fsSL ${API_AUTH_HEADER:+-H "$API_AUTH_HEADER"} "$RELEASE_API_URL" 2>/dev/null || true)"
elif has wget; then
  RELEASE_JSON="$(wget -qO- ${API_AUTH_HEADER:+--header="$API_AUTH_HEADER"} "$RELEASE_API_URL" 2>/dev/null || true)"
fi

if [ -n "$RELEASE_JSON" ]; then
  ASSET_NAMES="$(printf '%s' "$RELEASE_JSON" \
    | grep -oE '"name"[[:space:]]*:[[:space:]]*"[^"]+"' \
    | sed -E 's/.*"name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')"
  if [ -n "$ASSET_NAMES" ]; then
    if ! printf '%s\n' "$ASSET_NAMES" | grep -Fxq "$ARTIFACT"; then
      echo "ERROR: Platform ${os}/${arch} is not published for ${TAG}." >&2
      echo "Available artifacts:" >&2
      # Filter down to archive-style artifacts so the list is not cluttered
      # with checksum / signature files.
      ARCHIVE_LIST="$(printf '%s\n' "$ASSET_NAMES" | grep -E '\.(tar\.gz|zip)$' || true)"
      if [ -z "$ARCHIVE_LIST" ]; then
        ARCHIVE_LIST="$ASSET_NAMES"
      fi
      printf '%s\n' "$ARCHIVE_LIST" | sed 's/^/  - /' >&2
      echo "If you need this platform, please file an issue at ${ISSUES_URL}" >&2
      exit 1
    fi
  else
    echo "Warning: could not parse release metadata from GitHub API; continuing." >&2
  fi
else
  echo "Warning: could not reach GitHub API to validate platform (rate-limited or offline); continuing." >&2
fi

# --- download and install ---------------------------------------------------

echo "Installing ${BINARY_NAME} ${TAG} (${os}/${arch})..."

mkdir -p "$INSTALL_DIR"

TMP="$(mktemp)"
TMP_DIR="$(mktemp -d)"
TMP_CHECKSUMS="$(mktemp)"
TMP_BUNDLE="$(mktemp)"
trap 'rm -f "$TMP" "$TMP_CHECKSUMS" "$TMP_BUNDLE"; rm -rf "$TMP_DIR"' EXIT

download "$URL" "$TMP"
download "$CHECKSUMS_URL" "$TMP_CHECKSUMS"
download "$COSIGN_BUNDLE_URL" "$TMP_BUNDLE"

# --- verify cosign / Sigstore signature over SHA256SUMS

if ! has cosign; then
  echo "ERROR: 'cosign' is required to verify release signatures." >&2
  echo "Install it from https://docs.sigstore.dev/cosign/installation/ and retry." >&2
  exit 1
fi

echo "Verifying cosign signature for SHA256SUMS..."
if ! cosign verify-blob \
      --bundle "$TMP_BUNDLE" \
      --certificate-identity-regexp "$COSIGN_CERT_IDENTITY_RE" \
      --certificate-oidc-issuer "$COSIGN_OIDC_ISSUER" \
      "$TMP_CHECKSUMS" >/dev/null; then
  die "cosign signature verification failed for SHA256SUMS (${TAG})"
fi
echo "Signature verified."

CHECKSUMS="$(cat "$TMP_CHECKSUMS")"
EXPECTED="$(printf '%s\n' "$CHECKSUMS" | awk -v artifact="$ARTIFACT" '$2 == artifact { print $1; exit }')"
[ -n "$EXPECTED" ] || die "Could not find checksum for ${ARTIFACT}. If you need this platform, please file an issue at ${ISSUES_URL}"
ACTUAL="$(compute_sha256 "$TMP")"
[ "$ACTUAL" = "$EXPECTED" ] || die "Checksum mismatch for ${ARTIFACT}"
tar xzf "$TMP" -C "$TMP_DIR"
chmod +x "${TMP_DIR}/${INNER_BINARY}"
mv "${TMP_DIR}/${INNER_BINARY}" "${INSTALL_DIR}/${BINARY_NAME}"

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
