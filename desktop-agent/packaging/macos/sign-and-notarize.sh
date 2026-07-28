#!/usr/bin/env bash
#
# Sign, notarize, and staple a distributable DeskSync installer.
#
# Unsigned builds are fine on the machine that produced them and useless anywhere
# else: Gatekeeper blocks them, and — the part that actually breaks the product —
# macOS treats each unsigned rebuild as a different program, so screen-recording
# and keychain consent has to be granted again after every update. A stable
# Developer ID signature is what makes an upgrade invisible to the user.
#
# Nothing here can be faked without an Apple Developer account, so this script
# checks its inputs and says exactly what is missing rather than producing an
# artifact that looks signed and is not.
#
# Required:
#   DESKSYNC_SIGN_IDENTITY       "Developer ID Application: Name (TEAMID)"
#   DESKSYNC_INSTALLER_IDENTITY  "Developer ID Installer: Name (TEAMID)"
#
# Notarization (either form):
#   DESKSYNC_NOTARY_PROFILE      keychain profile from `notarytool store-credentials`
#   — or —
#   DESKSYNC_APPLE_ID, DESKSYNC_TEAM_ID, DESKSYNC_APP_PASSWORD
#
# Optional:
#   DESKSYNC_SKIP_NOTARIZE=1     sign only (useful for internal test builds)
#
# Usage: ./sign-and-notarize.sh [--host-only]

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
agent_root="$(cd "$here/../.." && pwd)"
dist="$agent_root/dist"

readonly APP="$dist/DeskSync.app"
readonly ENTITLEMENTS="$here/entitlements.plist"

fail() { echo "error: $*" >&2; exit 1; }

# ---- preflight -------------------------------------------------------------

[[ -n "${DESKSYNC_SIGN_IDENTITY:-}" ]] ||
    fail "DESKSYNC_SIGN_IDENTITY is not set.
       Available identities:
$(security find-identity -v -p codesigning | sed 's/^/         /')
       Get one from an Apple Developer account (Developer ID Application),
       or build an unsigned local package with ./build-pkg.sh"

[[ -n "${DESKSYNC_INSTALLER_IDENTITY:-}" ]] ||
    fail "DESKSYNC_INSTALLER_IDENTITY is not set (Developer ID Installer certificate)"

notarize=true
[[ "${DESKSYNC_SKIP_NOTARIZE:-}" == "1" ]] && notarize=false

if $notarize; then
    if [[ -z "${DESKSYNC_NOTARY_PROFILE:-}" ]]; then
        [[ -n "${DESKSYNC_APPLE_ID:-}" && -n "${DESKSYNC_TEAM_ID:-}" && -n "${DESKSYNC_APP_PASSWORD:-}" ]] ||
            fail "notarization needs either DESKSYNC_NOTARY_PROFILE, or all of
       DESKSYNC_APPLE_ID / DESKSYNC_TEAM_ID / DESKSYNC_APP_PASSWORD.
       Set DESKSYNC_SKIP_NOTARIZE=1 to sign without notarizing."
    fi
fi

# ---- build -----------------------------------------------------------------

"$here/build-app.sh" "$@"
version="$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$APP/Contents/Info.plist")"

# ---- sign the app ----------------------------------------------------------

echo "==> Signing $APP"
# Hardened runtime is required for notarization. The timestamp is what lets the
# signature stay valid after the certificate expires — without it, every build
# eventually starts failing Gatekeeper on users' machines.
codesign --force --deep \
    --sign "$DESKSYNC_SIGN_IDENTITY" \
    --options runtime \
    --timestamp \
    --entitlements "$ENTITLEMENTS" \
    "$APP"

echo "--> Verifying signature"
codesign --verify --deep --strict --verbose=2 "$APP"

# ---- package ---------------------------------------------------------------

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
mkdir -p "$staging/root/Applications"
cp -R "$APP" "$staging/root/Applications/"

component_pkg="$dist/DeskSync-component.pkg"
unsigned_pkg="$dist/DeskSync-$version-unsigned.pkg"
signed_pkg="$dist/DeskSync-$version.pkg"

echo "==> Building package"
pkgbuild \
    --root "$staging/root" \
    --identifier "com.desksync.agent" \
    --version "$version" \
    --install-location "/" \
    --scripts "$here/scripts" \
    --ownership recommended \
    "$component_pkg"

sed -e "s|@VERSION@|$version|g" \
    -e "s|@BUNDLE_ID@|com.desksync.agent|g" \
    -e "s|@COMPONENT_PKG@|$(basename "$component_pkg")|g" \
    "$here/distribution.xml.in" > "$staging/distribution.xml"

productbuild \
    --distribution "$staging/distribution.xml" \
    --resources "$here/resources" \
    --package-path "$dist" \
    "$unsigned_pkg"
rm -f "$component_pkg"

echo "==> Signing installer"
productsign --sign "$DESKSYNC_INSTALLER_IDENTITY" "$unsigned_pkg" "$signed_pkg"
rm -f "$unsigned_pkg"
pkgutil --check-signature "$signed_pkg"

# ---- notarize --------------------------------------------------------------

if ! $notarize; then
    echo
    echo "Built $signed_pkg (signed, NOT notarized)"
    echo "Gatekeeper will still warn on other Macs until it is notarized."
    exit 0
fi

echo "==> Submitting for notarization (this waits for Apple)"
if [[ -n "${DESKSYNC_NOTARY_PROFILE:-}" ]]; then
    xcrun notarytool submit "$signed_pkg" --keychain-profile "$DESKSYNC_NOTARY_PROFILE" --wait
else
    xcrun notarytool submit "$signed_pkg" \
        --apple-id "$DESKSYNC_APPLE_ID" \
        --team-id "$DESKSYNC_TEAM_ID" \
        --password "$DESKSYNC_APP_PASSWORD" \
        --wait
fi

echo "==> Stapling the ticket"
# Stapling attaches the ticket to the package so it validates offline. Without it
# a user with no network sees a Gatekeeper failure on a perfectly notarized build.
xcrun stapler staple "$signed_pkg"
xcrun stapler validate "$signed_pkg"

echo
echo "Built $signed_pkg — signed, notarized, stapled."
echo "Verify on a clean Mac: spctl --assess -vv --type install \"$signed_pkg\""
