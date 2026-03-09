#!/usr/bin/env bash
# arch installer for Linux/macOS
# Usage: curl -fsSL https://raw.githubusercontent.com/micsh/arch/main/install.sh | bash
#   or:  ./install.sh [install-dir]

set -euo pipefail

INSTALL_DIR="${1:-$HOME/.arch/bin}"
REPO="micsh/arch"

echo "🏗️  Installing arch..."

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS-$ARCH" in
    Linux-x86_64)   ASSET="arch-linux-x64" ;;
    Darwin-arm64)   ASSET="arch-macos-arm64" ;;
    Darwin-x86_64)  ASSET="arch-macos-x64" ;;
    *)
        echo "❌ Unsupported platform: $OS-$ARCH"
        echo "   Build from source: cargo install arch"
        exit 1
        ;;
esac

# Get latest release URL
URL="https://github.com/$REPO/releases/latest/download/$ASSET"
echo "   Downloading from $URL..."

# Create install directory
mkdir -p "$INSTALL_DIR"
DEST="$INSTALL_DIR/arch"

curl -fsSL "$URL" -o "$DEST"
chmod +x "$DEST"

# Suggest PATH addition
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
    SHELL_NAME="$(basename "$SHELL")"
    case "$SHELL_NAME" in
        zsh)  RC="$HOME/.zshrc" ;;
        bash) RC="$HOME/.bashrc" ;;
        fish) RC="$HOME/.config/fish/config.fish" ;;
        *)    RC="$HOME/.profile" ;;
    esac

    echo "export PATH=\"$INSTALL_DIR:\$PATH\"" >> "$RC"
    export PATH="$INSTALL_DIR:$PATH"
    echo "   Added $INSTALL_DIR to PATH in $RC"
fi

# Verify
VERSION=$("$DEST" --version 2>&1)
echo "✅ Installed $VERSION to $DEST"
echo ""
echo "   Restart your terminal, then run: arch --help"
