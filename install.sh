#!/bin/sh
# Builds hsbg-overlay and installs it for the current user:
#   ~/.local/bin/hsbg-overlay
#   ~/.local/share/hsbg-overlay/sidecar   (combat simulator, needs Node.js)
set -eu
cd "$(dirname "$0")"

cargo build --release
install -Dm755 target/release/hsbg-overlay "$HOME/.local/bin/hsbg-overlay"

share="${XDG_DATA_HOME:-$HOME/.local/share}/hsbg-overlay/sidecar"
mkdir -p "$share"
cp sidecar/sim.mjs sidecar/package.json sidecar/package-lock.json "$share/"
(cd "$share" && npm ci --omit=dev --no-fund --no-audit)

echo "installed: $HOME/.local/bin/hsbg-overlay"
