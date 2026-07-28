#!/usr/bin/env bash
#
# Build DeskSync.pkg — the double-clickable installer.
#
# Structure: a component package holding /Applications/DeskSync.app, wrapped in a
# distribution package so the installer can show a welcome and, more importantly, a
# conclusion pane telling the user the one thing left to do.
#
# Usage:
#   ./build-pkg.sh                 universal build
#   ./build-pkg.sh --host-only     current architecture only (faster, dev builds)
#
# Output: dist/DeskSync-<version>.pkg (unsigned — see sign-and-notarize.sh)

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
agent_root="$(cd "$here/../.." && pwd)"
dist="$agent_root/dist"

readonly BUNDLE_ID="com.desksync.agent"
readonly APP="$dist/DeskSync.app"

"$here/build-app.sh" "$@"

version="$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$APP/Contents/Info.plist")"
component_pkg="$dist/DeskSync-component.pkg"
product_pkg="$dist/DeskSync-$version.pkg"

# pkgbuild takes a directory that mirrors the destination filesystem, so stage the
# bundle under a root that contains nothing else. Copying (rather than pointing at
# dist/) keeps stray build output out of the payload.
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
mkdir -p "$staging/root/Applications"
cp -R "$APP" "$staging/root/Applications/"

echo "==> Building component package"
pkgbuild \
    --root "$staging/root" \
    --identifier "$BUNDLE_ID" \
    --version "$version" \
    --install-location "/" \
    --scripts "$here/scripts" \
    --ownership recommended \
    "$component_pkg"

echo "==> Building distribution package"
sed -e "s|@VERSION@|$version|g" \
    -e "s|@BUNDLE_ID@|$BUNDLE_ID|g" \
    -e "s|@COMPONENT_PKG@|$(basename "$component_pkg")|g" \
    "$here/distribution.xml.in" > "$staging/distribution.xml"

productbuild \
    --distribution "$staging/distribution.xml" \
    --resources "$here/resources" \
    --package-path "$dist" \
    "$product_pkg"

rm -f "$component_pkg"

echo
echo "Built $product_pkg"
du -h "$product_pkg" | awk '{print "  size: " $1}'
echo
echo "This package is UNSIGNED. macOS Gatekeeper will refuse to open it normally:"
echo "  • locally, right-click → Open, or: sudo installer -pkg \"$product_pkg\" -target /"
echo "  • to distribute it, run ./sign-and-notarize.sh with a Developer ID"
