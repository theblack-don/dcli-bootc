#!/usr/bin/env bash
set -euo pipefail

CONTAINER="dcli-bootc-builder"
PROJECT="$(cd "$(dirname "$0")" && pwd)"

GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${CYAN}━━━ dcli-bootc Dev Build ━━━${NC}"

# Ensure distrobox is installed
if ! command -v distrobox &>/dev/null; then
  echo -e "${RED}Error: distrobox not found.${NC}"
  echo "  Install it: sudo dnf install distrobox"
  exit 1
fi

# Create or reuse the builder container
if ! distrobox list 2>/dev/null | grep -q "$CONTAINER"; then
  echo -e "${YELLOW}Creating distrobox '${CONTAINER}' with Rust toolchain...${NC}"
  DBX_NON_INTERACTIVE=1 distrobox create \
    --name "$CONTAINER" \
    --image quay.io/fedora/fedora:41 \
    --pull --yes \
    --additional-flags "--userns=keep-id"
  distrobox enter "$CONTAINER" -- bash -c \
    "sudo dnf install -y rust cargo git cmake openssl-devel clang gcc make"
  echo -e "${GREEN}Builder container created.${NC}"
else
  echo -e "${GREEN}Using existing builder container '${CONTAINER}'.${NC}"
fi

# Build inside the container
echo -e "${CYAN}Building dcli-bootc (release)...${NC}"
distrobox enter "$CONTAINER" -- bash -c \
  "cd '${PROJECT}' && cargo build --release 2>&1"

# Check build result
BINARY="$PROJECT/target/release/dcli-bootc"
if [ -f "$BINARY" ]; then
  echo -e "${GREEN}✓ Build successful!${NC}"
  echo -e "  Binary: ${CYAN}$BINARY${NC}"
  echo -e "  Size:   $(du -h "$BINARY" | cut -f1)"
  echo ""
  echo -e "  Test it:"
  echo -e "    ${YELLOW}$BINARY --help${NC}"
  echo ""
  echo -e "  Install locally:"
  echo -e "    ${YELLOW}sudo cp '$BINARY' /usr/local/bin/dcli-bootc${NC}"
else
  echo -e "${RED}Build failed. Check the output above for errors.${NC}"
  exit 1
fi
