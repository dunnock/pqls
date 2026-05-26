#!/bin/sh
set -e

REPO="dunnock/pqls"
INSTALL_DIR="${PQLS_INSTALL:-/usr/local/bin}"
BIN_NAME="pqls"

# Detect OS
OS="$(uname -s)"
case "$OS" in
    Linux)  OS_NAME="linux" ;;
    Darwin) OS_NAME="darwin" ;;
    *)
        echo "Error: Unsupported OS: $OS" >&2
        exit 1
        ;;
esac

# Detect arch
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64)  ARCH_NAME="x86_64" ;;
    aarch64) ARCH_NAME="aarch64" ;;
    arm64)   ARCH_NAME="aarch64" ;;
    *)
        echo "Error: Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

ASSET_NAME="${BIN_NAME}-${OS_NAME}-${ARCH_NAME}.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ASSET_NAME}"

INSTALL_PATH="${INSTALL_DIR}/${BIN_NAME}"

echo "Downloading ${ASSET_NAME} from GitHub releases..."
if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$DOWNLOAD_URL" -o "/tmp/${ASSET_NAME}"
elif command -v wget >/dev/null 2>&1; then
    wget -q "$DOWNLOAD_URL" -O "/tmp/${ASSET_NAME}"
else
    echo "Error: curl or wget is required" >&2
    exit 1
fi

tar -xzf "/tmp/${ASSET_NAME}" -C /tmp
rm -f "/tmp/${ASSET_NAME}"

# Install (use sudo only if needed)
if [ -w "$INSTALL_DIR" ]; then
    mv "/tmp/${BIN_NAME}" "$INSTALL_PATH"
else
    echo "Installing to ${INSTALL_PATH} (requires sudo)..."
    sudo mv "/tmp/${BIN_NAME}" "$INSTALL_PATH"
fi

echo "Installed ${BIN_NAME} to ${INSTALL_PATH}"

# Verify
if command -v "$BIN_NAME" >/dev/null 2>&1; then
    "$BIN_NAME" --version
else
    "$INSTALL_PATH" --version
fi
