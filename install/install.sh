#!/bin/sh
# gvm (Go Version Manager) -- Linux and macOS installer
#
# Usage (one-liner):
#   curl -fsSL https://raw.githubusercontent.com/jhonsferg/gvm/main/install/install.sh | sh
#
# Customise with environment variables before piping:
#   GVM_INSTALL_DIR=$HOME/.local/bin GVM_VERSION=v1.0.0 curl -fsSL ... | sh

set -eu

REPO="${GVM_REPO:-jhonsferg/gvm}"
INSTALL_DIR="${GVM_INSTALL_DIR:-$HOME/.local/bin}"
GVM_VERSION="${GVM_VERSION:-latest}"

# Override base URLs for local/offline testing:
#   GVM_TEST_API_BASE=http://localhost:8765
#   GVM_TEST_DL_BASE=http://localhost:8765
API_BASE="${GVM_TEST_API_BASE:-https://api.github.com}"
DL_BASE="${GVM_TEST_DL_BASE:-https://github.com}"

# -- Terminal helpers ----------------------------------------------------------
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    C_CYAN='\033[0;36m'
    C_GREEN='\033[0;32m'
    C_YELLOW='\033[1;33m'
    C_RED='\033[0;31m'
    C_BOLD='\033[1m'
    C_RESET='\033[0m'
else
    C_CYAN='' C_GREEN='' C_YELLOW='' C_RED='' C_BOLD='' C_RESET=''
fi

step() { printf "  ${C_CYAN}->${C_RESET} %s\n" "$1"; }
ok()   { printf "  ${C_GREEN}v ${C_RESET} %s\n" "$1"; }
die()  { printf "  ${C_RED}x ${C_RESET} %s\n" "$1" >&2; exit 1; }

printf "\n  ${C_BOLD}${C_CYAN}gvm${C_RESET}${C_BOLD} -- Go Version Manager${C_RESET} installer\n\n"

# -- 1. Check required tools ---------------------------------------------------
need() {
    command -v "$1" > /dev/null 2>&1 || die "'$1' is required but not installed."
}
need curl

# -- 2. Detect OS and architecture ---------------------------------------------
OS="$(uname -s 2>/dev/null || echo unknown)"
case "$OS" in
    Linux)  PLATFORM="linux"  ;;
    Darwin) PLATFORM="darwin" ;;
    *)      die "Unsupported OS: $OS  (only Linux and macOS are supported)" ;;
esac

MACHINE="$(uname -m 2>/dev/null || echo unknown)"
case "$MACHINE" in
    x86_64 | amd64)           ARCH="x86_64"  ;;
    aarch64 | arm64 | armv8*) ARCH="aarch64" ;;
    *)                         die "Unsupported architecture: $MACHINE" ;;
esac

step "Detected platform: $PLATFORM-$ARCH"

# -- 3. Resolve version --------------------------------------------------------
if [ "$GVM_VERSION" = "latest" ]; then
    step "Fetching latest release from $API_BASE..."
    GVM_VERSION="$(
        curl -fsSL "$API_BASE/repos/$REPO/releases/latest" \
        | grep '"tag_name"' \
        | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
    )"
    [ -n "$GVM_VERSION" ] || die "Failed to resolve the latest version. Check your network."
fi

step "Installing gvm $GVM_VERSION"

# -- 4. Download binary --------------------------------------------------------
BINARY="gvm-$PLATFORM-$ARCH"
URL="$DL_BASE/$REPO/releases/download/$GVM_VERSION/$BINARY"
TMP_FILE="$(mktemp "${TMPDIR:-/tmp}/gvm-install.XXXXXX")"

trap 'rm -f "$TMP_FILE"' EXIT INT TERM

step "Downloading $BINARY..."
if ! curl -fSL --progress-bar "$URL" -o "$TMP_FILE"; then
    die "Download failed.\n  URL: $URL\n  Check that release $GVM_VERSION exists."
fi
chmod +x "$TMP_FILE"

# -- 5. Install binary ---------------------------------------------------------
mkdir -p "$INSTALL_DIR"
mv -f "$TMP_FILE" "$INSTALL_DIR/gvm"
ok "Installed to $INSTALL_DIR/gvm"

# -- 6. Run gvm setup ----------------------------------------------------------
# Run setup via full path so it works even before INSTALL_DIR is on PATH.
printf "\n  ${C_BOLD}Configuring shell environment...${C_RESET}\n\n"
"$INSTALL_DIR/gvm" setup

# -- 7. Summary ----------------------------------------------------------------
printf "\n  ${C_GREEN}${C_BOLD}gvm $GVM_VERSION installed and configured!${C_RESET}\n\n"
printf "  Next steps:\n\n"
printf "  1. Restart your shell (or source your profile), then install Go:\n"
printf "       ${C_CYAN}gvm install latest${C_RESET}\n"
printf "       ${C_CYAN}gvm use latest${C_RESET}\n\n"
printf "  Run ${C_CYAN}gvm doctor${C_RESET} to verify the setup.\n\n"
