#!/bin/sh
# Uninstalls hsbg-overlay and its sidecar data:
#   ~/.local/bin/hsbg-overlay
#   ~/.local/share/hsbg-overlay
#
# Pass --purge to also remove user configuration and cache:
#   ~/.config/hsbg-overlay
#   ~/.cache/hsbg-overlay
set -eu

bin="$HOME/.local/bin/hsbg-overlay"
share="${XDG_DATA_HOME:-$HOME/.local/share}/hsbg-overlay"
config="${XDG_CONFIG_HOME:-$HOME/.config}/hsbg-overlay"
cache="${XDG_CACHE_HOME:-$HOME/.cache}/hsbg-overlay"

removed=0

if [ -f "$bin" ]; then
    rm -f "$bin"
    echo "removed: $bin"
    removed=1
fi

if [ -d "$share" ]; then
    rm -rf "$share"
    echo "removed: $share"
    removed=1
fi

if [ "${1:-}" = "--purge" ]; then
    if [ -d "$config" ]; then
        rm -rf "$config"
        echo "removed config: $config"
        removed=1
    fi
    if [ -d "$cache" ]; then
        rm -rf "$cache"
        echo "removed cache: $cache"
        removed=1
    fi
fi

if [ "$removed" = 0 ]; then
    echo "nothing to remove"
else
    echo "uninstallation complete"
fi
