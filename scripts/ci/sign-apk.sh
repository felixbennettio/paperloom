#!/bin/bash
# Signs the release APK built by `tauri android build` into dist/.
#
# With the ANDROID_KEYSTORE_B64 / ANDROID_KEYSTORE_PASSWORD / ANDROID_KEY_ALIAS
# secrets set it signs with the release key. Without them it signs with a
# throwaway key and names the file "-test": installable for trying a build,
# but it cannot update an installation signed with the real key.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
VERSION=$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$ROOT/app/src-tauri/tauri.conf.json" | head -1)
UNSIGNED=$(find "$ROOT/app/src-tauri/gen/android/app/build/outputs/apk" -name '*-release-unsigned.apk' | head -1)
[ -n "$UNSIGNED" ] || { echo "no unsigned release APK found" >&2; exit 1; }
# arm64 for phones, x86_64 for the emulator tests.
ABI=${APK_ABI:-arm64}
BUILD_TOOLS="$ANDROID_HOME/build-tools/$(ls "$ANDROID_HOME/build-tools" | sort -V | tail -1)"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

if [ -n "${KEYSTORE_B64:-}" ]; then
  echo "$KEYSTORE_B64" | base64 -d > "$WORK/release.jks"
  NAME="Paperloom-$VERSION-android-$ABI.apk"
  ALIAS="${KEY_ALIAS:?ANDROID_KEY_ALIAS is not set}"
  PASS="${KEYSTORE_PASSWORD:?ANDROID_KEYSTORE_PASSWORD is not set}"
else
  echo "::warning::No Android release key in the secrets; signing with a throwaway test key."
  PASS=$(head -c 24 /dev/urandom | base64 | tr -dc 'A-Za-z0-9')
  ALIAS=test
  keytool -genkeypair -keystore "$WORK/release.jks" -storepass "$PASS" -keypass "$PASS" -alias "$ALIAS" \
    -keyalg RSA -keysize 2048 -validity 365 -dname "CN=Paperloom test build" >/dev/null 2>&1
  NAME="Paperloom-$VERSION-android-$ABI-test.apk"
fi

mkdir -p "$ROOT/dist"
"$BUILD_TOOLS/zipalign" -f -p 4 "$UNSIGNED" "$WORK/aligned.apk"
"$BUILD_TOOLS/apksigner" sign --v4-signing-enabled false --ks "$WORK/release.jks" --ks-pass "pass:$PASS" --key-pass "pass:$PASS" \
  --ks-key-alias "$ALIAS" --out "$ROOT/dist/$NAME" "$WORK/aligned.apk"
"$BUILD_TOOLS/apksigner" verify "$ROOT/dist/$NAME"
ls -lh "$ROOT/dist/$NAME"
