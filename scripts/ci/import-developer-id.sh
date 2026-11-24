#!/bin/bash
# Puts a Developer ID Application identity (a base64 .p12 in P12_B64, its
# password in P12_PASSWORD) into a throwaway keychain (KEYCHAIN, default in
# the job's temporary folder) on a macOS CI machine, so the DMG can be
# signed; Xcode Cloud's own signing identity is out of a script's reach.
# Does nothing without P12_B64.
set -euo pipefail

if [ -z "${P12_B64:-}" ]; then
  echo "No Developer ID certificate in the secrets; skipping"
  exit 0
fi
TMP="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
KEYCHAIN="${KEYCHAIN:-$TMP/signing.keychain-db}"
PASS=$(head -c 24 /dev/urandom | base64 | tr -dc 'A-Za-z0-9')
printf '%s' "$P12_B64" | base64 -D > "$TMP/developer-id.p12"
trap 'rm -f "$TMP/developer-id.p12"' EXIT
security create-keychain -p "$PASS" "$KEYCHAIN"
security set-keychain-settings -lut 21600 "$KEYCHAIN"
security unlock-keychain -p "$PASS" "$KEYCHAIN"
security import "$TMP/developer-id.p12" -k "$KEYCHAIN" -P "${P12_PASSWORD:-}" \
  -T /usr/bin/codesign -T /usr/bin/security -T /usr/bin/xcodebuild
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$PASS" "$KEYCHAIN" >/dev/null
# Searched before the login keychain, which stays in the list.
security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '"')
security find-identity -v -p codesigning "$KEYCHAIN"
