#!/usr/bin/env bash
set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo ""
echo "=== brain-dump installer (macOS) ==="
echo ""

# 1. Xcode Command Line Tools
if ! xcode-select -p &>/dev/null; then
    echo -e "${YELLOW}Installing Xcode Command Line Tools...${NC}"
    xcode-select --install
    echo "Press Enter once the Xcode CLT installation popup has finished..."
    read -r
fi
echo -e "${GREEN}✓ Xcode Command Line Tools${NC}"

# 2. Homebrew
if ! command -v brew &>/dev/null; then
    echo -e "${YELLOW}Installing Homebrew...${NC}"
    /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    # Add brew to PATH (Apple Silicon or Intel)
    if [[ -f /opt/homebrew/bin/brew ]]; then
        eval "$(/opt/homebrew/bin/brew shellenv)"
    else
        eval "$(/usr/local/bin/brew shellenv)"
    fi
fi
echo -e "${GREEN}✓ Homebrew${NC}"

# 3. Node.js
if ! command -v node &>/dev/null; then
    echo -e "${YELLOW}Installing Node.js 20...${NC}"
    brew install node@20
    brew link node@20 --force --overwrite
fi
echo -e "${GREEN}✓ Node.js $(node --version)${NC}"

# 4. Rust
if ! command -v rustup &>/dev/null; then
    echo -e "${YELLOW}Installing Rust...${NC}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi
# shellcheck source=/dev/null
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
echo -e "${GREEN}✓ Rust $(rustc --version)${NC}"

# 5. Build
echo ""
echo -e "${YELLOW}Installing npm dependencies and building...${NC}"
cd "$(dirname "$0")/brain-dump"
npm install
npm run tauri build

echo ""
echo -e "${GREEN}=== Terminé ===${NC}"
echo "App bundle : brain-dump/src-tauri/target/release/bundle/macos/Brain\\ Dump.app"
echo "DMG        : brain-dump/src-tauri/target/release/bundle/dmg/"
echo ""
echo "Pour lancer l'app, ouvre le .app ci-dessus ou double-clique sur le DMG."
echo ""
echo "Permissions requises au premier lancement :"
echo "  - Microphone (pour enregistrer la voix)"
echo "  - Accessibilité (pour détecter l'app active et simuler Cmd+V)"
