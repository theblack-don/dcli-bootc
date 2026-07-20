#!/usr/bin/env bash
set -euo pipefail

CONTAINER="dcli-bootc-builder"
PROJECT="$(cd "$(dirname "$0")" && pwd)"

GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${CYAN}━━━ dcli-bootc Dev Build (RPM pipeline, same as COPR) ━━━${NC}"

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
    "sudo dnf install -y rust cargo git cmake openssl-devel clang gcc make rpm-build"
  echo -e "${GREEN}Builder container created.${NC}"
else
  echo -e "${GREEN}Using existing builder container '${CONTAINER}'.${NC}"
fi

# Make sure rpm-build is available (containers created before it was needed)
distrobox enter "$CONTAINER" -- bash -c \
  'command -v rpmbuild >/dev/null 2>&1 || sudo dnf install -y rpm-build'

# Build the binary RPM exactly the way COPR does (dcli-bootc.spec + .copr/Makefile)
echo -e "${CYAN}Building dcli-bootc RPM (same pipeline as COPR)...${NC}"
distrobox enter "$CONTAINER" -- bash -c "cd '${PROJECT}' && make rpm"

# Locate the resulting binary RPM
RPM="$(ls -t "$PROJECT"/dist/dcli-bootc-[0-9]*.x86_64.rpm 2>/dev/null | head -1 || true)"
if [ -z "$RPM" ]; then
  echo -e "${RED}Build failed. Check the output above for errors.${NC}"
  exit 1
fi

# Extract the packaged binary — this is the exact binary the COPR pipeline
# produces, installed to /usr/local/bin (writable on atomic hosts).
echo -e "${CYAN}Extracting packaged binary...${NC}"
rm -rf "$PROJECT/dist/install"
mkdir -p "$PROJECT/dist/install"
distrobox enter "$CONTAINER" -- bash -c \
  "cd '${PROJECT}/dist/install' && rpm2cpio '${RPM}' | cpio -idm --quiet"
PACKAGED="$PROJECT/dist/install/usr/bin/dcli-bootc"
chmod 0755 "$PACKAGED"

echo -e "${GREEN}✓ RPM build successful!${NC}"
echo -e "  RPM:    ${CYAN}$RPM${NC}"
echo -e "  Binary: ${CYAN}$PACKAGED${NC}"
"$PACKAGED" --version
sha256sum "$PACKAGED" | cut -d' ' -f1 | sed 's/^/  sha256: /'
echo ""

read -rp "Install to /usr/local/bin/dcli-bootc? [y/N] " ans || ans=""
if [[ "${ans,,}" == "y" || "${ans,,}" == "yes" ]]; then
  sudo cp "$PACKAGED" /usr/local/bin/dcli-bootc
  echo -e "${GREEN}✓ Installed:${NC} $(command -v dcli-bootc)"
else
  echo "  Install later with:"
  echo -e "    ${YELLOW}sudo cp '$PACKAGED' /usr/local/bin/dcli-bootc${NC}"
fi
