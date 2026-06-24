#!/usr/bin/env bash
# flash.sh — Network-Scanner-3D ESP32-S3 setup + build + flash
# Run from the project root:  bash flash.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
FIRMWARE_DIR="$REPO_ROOT/firmware/esp32s3"
MAIN_C="$FIRMWARE_DIR/main/csi_capture.c"
IDF_DIR="$HOME/esp/esp-idf"
IDF_VERSION="v5.2.3"

# ── Colors ────────────────────────────────────────────────────────────────────
G='\033[0;32m'; Y='\033[1;33m'; R='\033[0;31m'; B='\033[0;34m'; N='\033[0m'
ok()   { echo -e "${G}✓ $*${N}"; }
info() { echo -e "${B}▶ $*${N}"; }
warn() { echo -e "${Y}⚠ $*${N}"; }
die()  { echo -e "${R}✗ $*${N}"; exit 1; }

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║   NETWORK SCANNER 3D — ESP32-S3 Setup & Flash Script    ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# ── 1. Check macOS prerequisites ──────────────────────────────────────────────
info "Checking prerequisites..."

if ! xcode-select -p &>/dev/null; then
    warn "Xcode Command Line Tools not found — installing..."
    xcode-select --install
    echo "  Re-run this script after the installer finishes."
    exit 0
fi
ok "Xcode CLT"

if ! command -v brew &>/dev/null; then
    warn "Homebrew not found — installing..."
    /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
fi
ok "Homebrew"

info "Installing build tools via Homebrew (cmake, ninja, dfu-util)..."
brew install cmake ninja dfu-util python3 2>/dev/null || true
ok "Build tools"

# ── 2. Install ESP-IDF ────────────────────────────────────────────────────────
if [ -f "$IDF_DIR/export.sh" ]; then
    ok "ESP-IDF already at $IDF_DIR"
else
    info "Cloning ESP-IDF $IDF_VERSION (~400 MB, this takes a few minutes)..."
    mkdir -p "$HOME/esp"
    git clone \
        --branch "$IDF_VERSION" \
        --depth 1 \
        --shallow-submodules \
        --recurse-submodules \
        https://github.com/espressif/esp-idf.git \
        "$IDF_DIR"
    ok "ESP-IDF cloned"

    info "Running ESP-IDF install for ESP32-S3 (downloads toolchain ~300 MB)..."
    "$IDF_DIR/install.sh" esp32s3
    ok "ESP-IDF toolchain installed"
fi

# Source the IDF environment
# shellcheck disable=SC1091
source "$IDF_DIR/export.sh" 2>/dev/null
ok "ESP-IDF environment loaded"

# ── 3. Detect ESP32 port ──────────────────────────────────────────────────────
info "Looking for ESP32-S3 on USB..."

PORT=""
for candidate in \
    /dev/cu.usbserial-* \
    /dev/cu.SLAB_USBtoUART \
    /dev/cu.usbmodem* \
    /dev/cu.wchusbserial* ; do
    if ls "$candidate" 2>/dev/null | head -1 | grep -q "."; then
        PORT=$(ls "$candidate" 2>/dev/null | head -1)
        break
    fi
done

if [ -z "$PORT" ]; then
    echo ""
    echo "  Available serial ports:"
    ls /dev/cu.* 2>/dev/null | grep -v Bluetooth || echo "  (none found)"
    echo ""
    read -rp "  Enter ESP32 port manually (e.g. /dev/cu.usbserial-0001): " PORT
fi
[ -z "$PORT" ] && die "No port specified."
ok "ESP32 port: $PORT"

# ── 4. WiFi credentials ───────────────────────────────────────────────────────
echo ""
info "WiFi configuration (for ESP32-S3 to reach ns3d-server)"
read -rp "  WiFi SSID     : " WIFI_SSID
read -rsp "  WiFi Password : " WIFI_PASS; echo ""

# ── 5. Detect laptop IP ───────────────────────────────────────────────────────
info "Detecting laptop IP on local network..."
SERVER_IP=$(ipconfig getifaddr en0 2>/dev/null || ipconfig getifaddr en1 2>/dev/null || echo "")
if [ -z "$SERVER_IP" ]; then
    echo ""
    echo "  Could not auto-detect IP. Available interfaces:"
    ifconfig | grep "inet " | grep -v 127.0.0.1 | awk '{print "  " $2}'
    read -rp "  Enter your laptop IP : " SERVER_IP
fi
ok "Server IP: $SERVER_IP"

# ── 6. Patch firmware source ──────────────────────────────────────────────────
info "Patching firmware with your credentials..."

# Create a working copy so the original stays clean
WORK_C="$FIRMWARE_DIR/main/csi_capture_patched.c"
cp "$MAIN_C" "$WORK_C"

sed -i '' \
    -e "s|#define WIFI_SSID.*|#define WIFI_SSID        \"$WIFI_SSID\"|" \
    -e "s|#define WIFI_PASS.*|#define WIFI_PASS        \"$WIFI_PASS\"|" \
    -e "s|#define SERVER_IP.*|#define SERVER_IP        \"$SERVER_IP\"|" \
    "$WORK_C"

# Use patched file as main source (swap temporarily)
mv "$MAIN_C" "${MAIN_C}.orig"
cp "$WORK_C" "$MAIN_C"

ok "Firmware patched (SSID=$WIFI_SSID, server=$SERVER_IP)"

# ── 7. Build ──────────────────────────────────────────────────────────────────
info "Building firmware (first build ~2 min)..."
cd "$FIRMWARE_DIR"

idf.py set-target esp32s3
idf.py \
    -DSDKCONFIG_DEFAULTS=sdkconfig.defaults \
    build

ok "Build complete"

# ── 8. Flash ─────────────────────────────────────────────────────────────────
echo ""
info "Ready to flash to $PORT"
echo "  Hold the BOOT button on the ESP32-S3 if you see a flash error."
echo ""
read -rp "  Press ENTER to flash (or Ctrl-C to cancel)..."

idf.py -p "$PORT" flash

ok "Firmware flashed!"

# Restore original source
mv "${MAIN_C}.orig" "$MAIN_C"
rm -f "$WORK_C"

# ── 9. Done ───────────────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║                    FLASH COMPLETE ✓                     ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "  ESP32-S3 will now:"
echo "  1. Connect to '$WIFI_SSID'"
echo "  2. Stream CSI frames to $SERVER_IP:5006 (UDP)"
echo ""
echo "  Next steps:"
echo "  ┌─────────────────────────────────────────────────────┐"
echo "  │  # In your project folder:                         │"
echo "  │  cargo run -p ns3d-server                          │"
echo "  │  # Then open http://localhost:3000                  │"
echo "  └─────────────────────────────────────────────────────┘"
echo ""
echo "  To monitor ESP32 serial output:"
echo "    idf.py -p $PORT monitor"
echo ""

# Offer to start server
read -rp "  Start ns3d-server now? (y/N) " START
if [[ "$START" =~ ^[Yy]$ ]]; then
    cd "$REPO_ROOT"
    if command -v cargo &>/dev/null; then
        cargo run -p ns3d-server
    else
        warn "Rust/cargo not found. Install from https://rustup.rs then run:"
        echo "  cargo run -p ns3d-server"
    fi
fi
