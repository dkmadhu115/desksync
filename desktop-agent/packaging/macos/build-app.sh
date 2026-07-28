#!/usr/bin/env bash
#
# Build DeskSync.app — the app bundle the installer ships.
#
# Why a bundle rather than a bare Unix binary: macOS ties screen-recording and
# accessibility consent to an executable's identity, and shows that identity to
# the user in System Settings. A loose binary appears there as "desksync-agent"
# with a generic icon and gets a fresh identity every time it is replaced; a
# bundle with a stable id and signature appears as "DeskSync" and keeps its
# grants across upgrades.
#
# Usage:
#   ./build-app.sh                 universal (arm64 + x86_64), release
#   ./build-app.sh --host-only     current architecture only (faster, dev builds)
#
# Output: dist/DeskSync.app

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
agent_root="$(cd "$here/../.." && pwd)"
dist="$agent_root/dist"

readonly BINARY="desksync-agent"
readonly BUNDLE_ID="com.desksync.agent"
readonly APP="$dist/DeskSync.app"

host_only=false
[[ "${1:-}" == "--host-only" ]] && host_only=true

# The bundle version must match what the binary reports, or `status` and the
# installer will disagree about what is installed.
version="$(
    awk '/^\[workspace\.package\]/{f=1;next} f && /^version *=/{gsub(/[" ]/,"",$3); print $3; exit}' \
        "$agent_root/Cargo.toml"
)"
[[ -n "$version" ]] || { echo "error: could not read version from Cargo.toml" >&2; exit 1; }

echo "==> Building DeskSync $version"

cd "$agent_root"
if $host_only; then
    echo "--> release build (host architecture only)"
    cargo build --release -p desksync-agent --features native
    binary_path="$agent_root/target/release/$BINARY"
else
    # Ship one binary that runs natively on both Apple silicon and Intel. Anything
    # else means either two downloads or Rosetta, and Rosetta cannot be used for a
    # screen-capture agent without a performance cost the user will see.
    for target in aarch64-apple-darwin x86_64-apple-darwin; do
        if ! rustup target list --installed | grep -qx "$target"; then
            echo "error: missing rust target $target" >&2
            echo "       run: rustup target add $target" >&2
            echo "       or build with --host-only for a dev build" >&2
            exit 1
        fi
        echo "--> release build for $target"
        cargo build --release -p desksync-agent --features native --target "$target"
    done

    echo "--> lipo → universal binary"
    mkdir -p "$dist"
    binary_path="$dist/$BINARY-universal"
    lipo -create -output "$binary_path" \
        "$agent_root/target/aarch64-apple-darwin/release/$BINARY" \
        "$agent_root/target/x86_64-apple-darwin/release/$BINARY"
fi

echo "==> Assembling $(basename "$APP")"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

install -m 0755 "$binary_path" "$APP/Contents/MacOS/$BINARY"

sed -e "s|@VERSION@|$version|g" \
    -e "s|@BUNDLE_ID@|$BUNDLE_ID|g" \
    -e "s|@EXECUTABLE@|$BINARY|g" \
    -e "s|@COPYRIGHT@|Copyright © $(date +%Y) DeskSync|g" \
    "$here/Info.plist.in" > "$APP/Contents/Info.plist"

# An ad-hoc signature is not a substitute for a Developer ID, but it does give the
# bundle a stable-enough identity for local testing and is required on Apple
# silicon for the binary to run at all.
echo "==> Ad-hoc signing (for local use)"
codesign --force --sign - --timestamp=none "$APP" 2>/dev/null ||
    echo "warning: ad-hoc signing failed; the app may not launch" >&2

echo
echo "Built $APP ($version)"
lipo -archs "$APP/Contents/MacOS/$BINARY" 2>/dev/null | sed 's/^/  architectures: /'
echo
echo "For a distributable build, sign it: ./sign-and-notarize.sh"
