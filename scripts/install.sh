#!/bin/bash
# Treeline CLI Installer
# Usage: curl -fsSL https://treeline.money/install.sh | sh
#
# Installs the Treeline CLI to ~/.treeline/bin/tl

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

REPO="treeline-money/treeline"
INSTALL_DIR="$HOME/.treeline/bin"
BINARY_NAME="tl"

# Detect OS and architecture
detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux*)
            case "$ARCH" in
                x86_64)
                    PLATFORM="linux"
                    ARTIFACT="tl-linux-x64"
                    ;;
                *)
                    echo -e "${RED}Error: Unsupported architecture: $ARCH${NC}"
                    echo "Supported: x86_64"
                    exit 1
                    ;;
            esac
            ;;
        Darwin*)
            case "$ARCH" in
                arm64)
                    PLATFORM="macos"
                    ARTIFACT="tl-macos-arm64"
                    ;;
                x86_64)
                    # Intel Mac - check if we have a binary for it
                    echo -e "${YELLOW}Note: Intel Mac support is limited. Trying arm64 binary with Rosetta...${NC}"
                    PLATFORM="macos"
                    ARTIFACT="tl-macos-arm64"
                    ;;
                *)
                    echo -e "${RED}Error: Unsupported architecture: $ARCH${NC}"
                    exit 1
                    ;;
            esac
            ;;
        MINGW*|MSYS*|CYGWIN*)
            echo -e "${YELLOW}For Windows, please use PowerShell:${NC}"
            echo "  irm https://treeline.money/install.ps1 | iex"
            exit 1
            ;;
        *)
            echo -e "${RED}Error: Unsupported operating system: $OS${NC}"
            echo "Supported: Linux, macOS"
            echo "For Windows, use: irm https://treeline.money/install.ps1 | iex"
            exit 1
            ;;
    esac
}

# Get latest release version
get_latest_version() {
    if command -v curl &> /dev/null; then
        VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    elif command -v wget &> /dev/null; then
        VERSION=$(wget -qO- "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    else
        echo -e "${RED}Error: curl or wget is required${NC}"
        exit 1
    fi

    if [ -z "$VERSION" ]; then
        echo -e "${RED}Error: Could not determine latest version${NC}"
        exit 1
    fi
}

# Download and install
install() {
    echo -e "${GREEN}Installing Treeline CLI...${NC}"
    echo ""

    detect_platform
    get_latest_version

    echo "  Platform: $PLATFORM ($ARCH)"
    echo "  Version:  $VERSION"
    echo "  Install:  $INSTALL_DIR/$BINARY_NAME"
    echo ""

    # Create install directory
    mkdir -p "$INSTALL_DIR"

    # Download URL
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$ARTIFACT"

    echo -e "${YELLOW}Downloading...${NC}"

    # Download binary
    if command -v curl &> /dev/null; then
        curl -fsSL "$DOWNLOAD_URL" -o "$INSTALL_DIR/$BINARY_NAME"
    else
        wget -q "$DOWNLOAD_URL" -O "$INSTALL_DIR/$BINARY_NAME"
    fi

    # Make executable
    chmod +x "$INSTALL_DIR/$BINARY_NAME"

    echo -e "${GREEN}Installed successfully!${NC}"
    echo ""

    # Add to PATH if not already there
    if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
        # Detect shell and profile file
        SHELL_NAME=$(basename "$SHELL")
        case "$SHELL_NAME" in
            zsh)
                PROFILE="$HOME/.zshrc"
                ;;
            bash)
                if [ -f "$HOME/.bash_profile" ]; then
                    PROFILE="$HOME/.bash_profile"
                else
                    PROFILE="$HOME/.bashrc"
                fi
                ;;
            *)
                PROFILE="$HOME/.profile"
                ;;
        esac

        # Add to profile if not already there
        PATH_EXPORT='export PATH="$HOME/.treeline/bin:$PATH"'
        if ! grep -q ".treeline/bin" "$PROFILE" 2>/dev/null; then
            echo "" >> "$PROFILE"
            echo "# Treeline CLI" >> "$PROFILE"
            echo "$PATH_EXPORT" >> "$PROFILE"
            echo -e "${GREEN}Added to PATH in $PROFILE${NC}"
            echo ""
            echo "Restart your terminal or run:"
            echo "  source $PROFILE"
            echo ""
        fi

        # Also export for current session
        export PATH="$INSTALL_DIR:$PATH"
    fi

    echo "Run 'tl --help' to get started."
}

install
