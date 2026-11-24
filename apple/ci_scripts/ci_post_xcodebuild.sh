#!/bin/bash
# Xcode Cloud, after xcodebuild. In the Developer ID workflow
# (PAPERLOOM_DIRECT=1, archive with Developer ID distribution): notarizes
# the app, puts it in a DMG (signed when the secrets hold a Developer ID
# identity), notarizes that, and for a v* tag adds it to the GitHub release.
# Files a script makes are deleted with the build, so without a tag the DMG
# is only checked, not kept.
#
# Workflow environment (secrets marked as such): APP_STORE_CONNECT_KEY_ID,
# APP_STORE_CONNECT_ISSUER_ID, APP_STORE_CONNECT_PRIVATE_KEY (secret);
# optional DEVELOPER_ID_P12_B64 and DEVELOPER_ID_P12_PASSWORD (secret);
# GITHUB_RELEASE_TOKEN (secret) for releases.
set -euo pipefail

[ "${PAPERLOOM_DIRECT:-}" = 1 ] || exit 0
[ "${CI_XCODEBUILD_ACTION:-}" = archive ] || exit 0
# A failed archive already fails the build.
[ "${CI_XCODEBUILD_EXIT_CODE:-1}" = 0 ] || exit 0

REPO="${CI_PRIMARY_REPOSITORY_PATH:?not running in Xcode Cloud}"
EXPORT="${CI_DEVELOPER_ID_SIGNED_APP_PATH:?set the distribution of the archive action to Developer ID}"
echo "Developer ID export: $EXPORT"
APP=$EXPORT
if [ "${EXPORT%.app}" = "$EXPORT" ]; then
  APP=$(find "$EXPORT" -maxdepth 2 -name '*.app' -type d | head -1)
fi
[ -d "$APP" ] || { echo "no app in $EXPORT" >&2; exit 1; }
VERSION=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")

WORK=$(mktemp -d)
cleanup() {
  rm -rf "$WORK"
  if [ -f "$WORK.keychain-db" ]; then security delete-keychain "$WORK.keychain-db" || true; fi
}
trap cleanup EXIT
printf '%s\n' "${APP_STORE_CONNECT_PRIVATE_KEY:?}" > "$WORK/AuthKey.p8"
notarize() {
  xcrun notarytool submit "$1" --wait --timeout 45m \
    --key "$WORK/AuthKey.p8" --key-id "${APP_STORE_CONNECT_KEY_ID:?}" --issuer "${APP_STORE_CONNECT_ISSUER_ID:?}"
}

echo "==> Notarize the app"
codesign -dv "$APP" 2>&1 | grep -E 'Authority=Developer ID|flags=.*runtime'
ditto -c -k --keepParent "$APP" "$WORK/Paperloom.zip"
notarize "$WORK/Paperloom.zip"
xcrun stapler staple "$APP"

echo "==> DMG"
NAME="Paperloom-$VERSION-macos-universal"
mkdir -p "$WORK/dmg" "$WORK/dist"
cp -R "$APP" "$WORK/dmg/"
ln -s /Applications "$WORK/dmg/Applications"
DMG="$WORK/dist/$NAME.dmg"
hdiutil create -volname "Paperloom $VERSION" -srcfolder "$WORK/dmg" -ov -format UDZO "$DMG"
if [ -n "${DEVELOPER_ID_P12_B64:-}" ]; then
  KEYCHAIN="$WORK.keychain-db" P12_B64="$DEVELOPER_ID_P12_B64" P12_PASSWORD="${DEVELOPER_ID_P12_PASSWORD:-}" \
    "$REPO/scripts/ci/import-developer-id.sh"
  IDENTITY=$(security find-identity -v -p codesigning "$WORK.keychain-db" |
    sed -n "s/.*\"\(Developer ID Application: .*(${CI_TEAM_ID:-})\)\"/\1/p" | head -1)
  codesign --sign "${IDENTITY:?no Developer ID Application identity for team ${CI_TEAM_ID:-} in the .p12}" \
    --keychain "$WORK.keychain-db" --timestamp "$DMG"
else
  echo "No Developer ID certificate in the secrets; the DMG itself is not signed"
fi
notarize "$DMG"
xcrun stapler staple "$DMG"

if [ -n "${CI_TAG:-}" ]; then
  python3 "$REPO/scripts/ci/github_release.py" "$CI_TAG" "$DMG"
else
  echo "Built and notarized $NAME.dmg; it is kept only for v* tags"
fi
