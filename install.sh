#!/bin/sh
# Builds hsbg-overlay and installs it for the current user:
#   ~/.local/bin/hsbg-overlay
#   ~/.local/share/hsbg-overlay/sidecar   (combat simulator, needs Node.js)
#
# Safe to rerun: it pulls new commits from the tracked branch first, and only
# replaces what actually changed. A `cap_sys_ptrace` already set on the
# installed binary (Wine + ptrace_scope=1, see README) is reapplied after it
# is replaced.
set -eu
cd "$(dirname "$0")"

# Verify prerequisites before doing anything.
if ! command -v cargo >/dev/null 2>&1; then
    echo "error: 'cargo' was not found in PATH." >&2
    echo "       Rust (stable) is required to build hsbg-overlay." >&2
    echo "       Install it from https://rustup.rs or using your system package manager." >&2
    exit 1
fi

if ! command -v node >/dev/null 2>&1; then
    echo "error: 'node' was not found in PATH." >&2
    echo "       Node.js (>= 18) is required for the combat simulator sidecar." >&2
    echo "       Please install Node.js." >&2
    exit 1
fi

if ! command -v npm >/dev/null 2>&1; then
    echo "error: 'npm' was not found in PATH." >&2
    echo "       npm is required to install simulator sidecar dependencies." >&2
    echo "       Please install npm." >&2
    exit 1
fi

node_version=$(node -v 2>/dev/null || echo "")
node_major=$(echo "$node_version" | sed -E 's/^v([0-9]+).*/\1/')
if [ -n "$node_major" ] && [ "$node_major" -lt 18 ]; then
    echo "warning: Node.js version $node_version is older than recommended (>= 18); simulator might not work properly" >&2
fi

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

bin_dir="$HOME/.local/bin"
bin="$bin_dir/hsbg-overlay"
mkdir -p "$bin_dir"

# Replacing the file drops any capability it had (README: Wine with
# ptrace_scope=1 needs `setcap cap_sys_ptrace=eip` on the overlay), so note
# whether the installed binary carries it and reapply it afterwards.
ptrace_cap=0
if [ -f "$bin" ] && command -v getcap >/dev/null 2>&1; then
    case "$(getcap "$bin" 2>/dev/null || true)" in
        *cap_sys_ptrace*) ptrace_cap=1 ;;
    esac
fi

if cmp -s target/release/hsbg-overlay "$bin"; then
    echo "up to date: $bin"
    updated=0
else
    install -Dm755 target/release/hsbg-overlay "$bin"
    echo "installed: $bin ($(git rev-parse --short HEAD 2>/dev/null || echo local build))"
    updated=1
fi

if [ "$ptrace_cap" = 1 ] && [ "$updated" = 1 ]; then
    if command -v setcap >/dev/null 2>&1 && setcap cap_sys_ptrace=eip "$bin" 2>/dev/null; then
        echo "reapplied: cap_sys_ptrace on $bin"
    elif command -v sudo >/dev/null 2>&1 && sudo -n setcap cap_sys_ptrace=eip "$bin" 2>/dev/null; then
        echo "reapplied: cap_sys_ptrace on $bin (via sudo)"
    else
        echo "warning: the previous binary had cap_sys_ptrace and the new one does not; reapply it with:" >&2
        echo "         sudo setcap cap_sys_ptrace=eip $bin" >&2
    fi
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

config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/hsbg-overlay"
mkdir -p "$config_dir"

# Check if ~/.local/bin is in PATH.
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *)
        echo "note: $bin_dir is not in your PATH. Consider adding it to your shell configuration (~/.bashrc, ~/.zshrc):"
        echo "      export PATH=\"\$HOME/.local/bin:\$PATH\""
        ;;
esac

# A running overlay keeps the old code until it is restarted.
if [ "$updated" = 1 ] && pgrep -x hsbg-overlay >/dev/null; then
    echo "hsbg-overlay is running the old version; restart it to apply the update"
fi
