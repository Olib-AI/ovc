#!/usr/bin/env bash
# Sign, package, notarize, and staple an OVC macOS application bundle.

set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "usage: $0 <OVC.app> <output.dmg>" >&2
    exit 2
fi

: "${APPLE_CERTIFICATE_B64:?APPLE_CERTIFICATE_B64 is required}"
: "${APPLE_CERTIFICATE_PASSWORD:?APPLE_CERTIFICATE_PASSWORD is required}"
: "${APPLE_ID:?APPLE_ID is required}"
: "${APPLE_APP_PASSWORD:?APPLE_APP_PASSWORD is required}"
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID is required}"

APP_PATH="$1"
OUTPUT_DMG="$2"
TEMP_DIR=$(mktemp -d)
KEYCHAIN_PATH="${TEMP_DIR}/ovc-build.keychain-db"
KEYCHAIN_PASSWORD=$(openssl rand -hex 16)

cleanup() {
    security delete-keychain "$KEYCHAIN_PATH" >/dev/null 2>&1 || true
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

echo "$APPLE_CERTIFICATE_B64" | base64 --decode > "${TEMP_DIR}/certificate.p12"
security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security set-keychain-settings -lut 21600 "$KEYCHAIN_PATH"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security import "${TEMP_DIR}/certificate.p12" \
    -P "$APPLE_CERTIFICATE_PASSWORD" -A -t cert -f pkcs12 -k "$KEYCHAIN_PATH"
security set-key-partition-list \
    -S apple-tool:,apple: -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security list-keychains -d user -s "$KEYCHAIN_PATH" login.keychain-db

IDENTITY=$(security find-identity -v -p codesigning "$KEYCHAIN_PATH" \
    | awk -F'"' '/Developer ID Application/ { print $2; exit }')
if [ -z "$IDENTITY" ]; then
    echo "Developer ID Application identity not found" >&2
    exit 1
fi

codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP_PATH"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"

hdiutil create -volname OVC -srcfolder "$APP_PATH" \
    -ov -format UDZO "$OUTPUT_DMG"
codesign --force --timestamp --sign "$IDENTITY" "$OUTPUT_DMG"

xcrun notarytool submit "$OUTPUT_DMG" \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_APP_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    --wait
xcrun stapler staple "$OUTPUT_DMG"
xcrun stapler validate "$OUTPUT_DMG"
