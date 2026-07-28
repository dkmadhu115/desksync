#!/usr/bin/env bash
#
# Remove DeskSync from this Mac.
#
# A product that cannot be cleanly removed is a product people resent installing,
# and a half-removed launchd job is worse than either state: it keeps restarting
# with no app behind it. So this stops the service first, then removes files.
#
# By default your credentials, device identity, and logs are kept, so reinstalling
# picks up where you left off. Use --purge to remove those too.
#
# Usage:
#   ./uninstall.sh            remove the app and service, keep local state
#   ./uninstall.sh --purge    remove everything, including stored credentials

set -uo pipefail

readonly APP="/Applications/DeskSync.app"
readonly BINARY="$APP/Contents/MacOS/desksync-agent"
readonly LINK="/usr/local/bin/desksync"
readonly LABEL="com.desksync.agent"
readonly PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
readonly STATE="$HOME/Library/Application Support/desksync"
readonly LOGS="$HOME/Library/Logs/DeskSync"

purge=false
[[ "${1:-}" == "--purge" ]] && purge=true

echo "==> Stopping the background service"
if [[ -x "$BINARY" ]]; then
    # Prefer the agent's own uninstall: it boots the job out of launchd and removes
    # the entry, which is the only order that does not orphan a running process.
    "$BINARY" service uninstall 2>/dev/null && echo "    stopped and removed the service entry"
fi
# Fall back to launchctl in case the binary is already gone.
if [[ -f "$PLIST" ]]; then
    launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null
    launchctl unload -w "$PLIST" 2>/dev/null
    rm -f "$PLIST" && echo "    removed $PLIST"
fi
pkill -f "$BINARY" 2>/dev/null && echo "    stopped a running agent process"

echo "==> Removing files"
for path in "$LINK" "$APP"; do
    if [[ -e "$path" || -L "$path" ]]; then
        if rm -rf "$path" 2>/dev/null; then
            echo "    removed $path"
        else
            echo "    need admin rights for $path — retrying with sudo"
            sudo rm -rf "$path" && echo "    removed $path"
        fi
    fi
done

# The package receipt has to go, or macOS still reports DeskSync as installed and a
# reinstall of the same version can skip the payload.
if pkgutil --pkgs | grep -qx "$LABEL"; then
    sudo pkgutil --forget "$LABEL" >/dev/null && echo "    forgot the package receipt"
fi

if $purge; then
    echo "==> Removing local state"
    rm -rf "$STATE" && echo "    removed $STATE"
    rm -rf "$LOGS" && echo "    removed $LOGS"
    # Credentials live in the login keychain, not in a file, so they survive
    # deleting the state directory and have to be deleted explicitly.
    if security delete-generic-password -s "$LABEL" >/dev/null 2>&1; then
        echo "    removed stored credentials from the keychain"
    fi
else
    echo
    echo "Kept your sign-in and device identity:"
    echo "  $STATE"
    echo "  keychain entry \"$LABEL\""
    echo "Re-run with --purge to remove them."
fi

echo
echo "DeskSync removed."
echo "macOS keeps its own record of the permissions you granted; you can clear those"
echo "in System Settings → Privacy & Security → Screen Recording / Accessibility."
