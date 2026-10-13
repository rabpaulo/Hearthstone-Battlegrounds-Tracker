#!/bin/sh
# Builds hsbg-overlay and installs it for the current user:
#   ~/.local/bin/hsbg-overlay
#   ~/.local/share/hsbg-overlay/sidecar   (combat simulator, needs Node.js)
#
# Safe to rerun: it pulls new commits from the tracked branch first, and only
# replaces what actually changed.
set -eu
cd "$(dirname "$0")"

# Pull new commits, if this is a clean checkout with an upstream.
if git rev-parse --abbrev-ref '@{u}' >/dev/null 2>&1; then
    git fetch --quiet
    behind=$(git rev-list --count 'HEAD..@{u}')
    if [ "$behind" -gt 0 ]; then
        if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
            echo "warning: $behind new commit(s) upstream, but local changes are in the way; not pulling" >&2
        else
            echo "pulling $behind new commit(s)"
            git pull --ff-only --quiet
        fi
    fi
fi

cargo build --release

bin="$HOME/.local/bin/hsbg-overlay"
if cmp -s target/release/hsbg-overlay "$bin"; then
    echo "up to date: $bin"
    updated=0
else
    install -Dm755 target/release/hsbg-overlay "$bin"
    echo "installed: $bin ($(git rev-parse --short HEAD 2>/dev/null || echo local build))"
    updated=1
fi

share="${XDG_DATA_HOME:-$HOME/.local/share}/hsbg-overlay/sidecar"
mkdir -p "$share"
deps_changed=0
for f in sim.mjs package.json package-lock.json; do
    if ! cmp -s "sidecar/$f" "$share/$f"; then
        cp "sidecar/$f" "$share/"
        [ "$f" = sim.mjs ] || deps_changed=1
        updated=1
    fi
done
if [ "$deps_changed" = 1 ] || [ ! -d "$share/node_modules" ]; then
    (cd "$share" && npm ci --omit=dev --no-fund --no-audit)
fi

# A running overlay keeps the old code until it is restarted.
if [ "$updated" = 1 ] && pgrep -x hsbg-overlay >/dev/null; then
    echo "hsbg-overlay is running the old version; restart it to apply the update"
fi
