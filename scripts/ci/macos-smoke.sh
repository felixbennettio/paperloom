#!/bin/bash
# Installs the notarized Mac app the way a person would and starts it:
# downloads the zip (Xcode Cloud's "stapled notarized archive" from a Mac
# Direct build, passed as a short-lived link), checks its signature and
# Gatekeeper's verdict, copies it to /Applications, opens it, and keeps
# screenshots with a summary.
#
#   scripts/ci/macos-smoke.sh <link to the app's zip> <output folder>
#
# Fails when the app does not pass Gatekeeper or is gone within 30 seconds;
# a screenshot the build machine does not allow is noted, not fatal.
set -uo pipefail

URL=${1:?link to the notarized app zip}
OUT=${2:?output folder}
mkdir -p "$OUT"
WORK=$(mktemp -d)
failed=0
note() { echo "$*" | tee -a "$OUT/summary.txt"; }
fail() { note "FAILED: $*"; failed=1; }
# Runs a command for at most $1 seconds: a permission prompt on the build
# machine would otherwise wait for a click forever. macOS has no `timeout`.
limit() {
  local seconds=$1; shift
  "$@" &
  local pid=$!
  ( sleep "$seconds"; kill "$pid" 2>/dev/null ) &
  local watchdog=$!
  wait "$pid"
  local status=$?
  kill "$watchdog" 2>/dev/null
  return $status
}

curl -fsSL --retry 3 -o "$WORK/app.zip" "$URL" || { fail "could not download the app"; exit 1; }
ditto -x -k "$WORK/app.zip" "$WORK/app" || { fail "could not unzip the app"; exit 1; }
APP=$(find "$WORK/app" -maxdepth 2 -name "*.app" -type d | head -1)
[ -n "$APP" ] || { fail "no .app in the zip"; exit 1; }
note "app: $(basename "$APP"), version $(defaults read "$APP/Contents/Info.plist" CFBundleShortVersionString 2>/dev/null) ($(defaults read "$APP/Contents/Info.plist" CFBundleVersion 2>/dev/null))"

note "--- codesign"
codesign --verify --deep --strict --verbose=2 "$APP" >> "$OUT/summary.txt" 2>&1 || fail "codesign does not verify"
codesign -dv "$APP" 2>&1 | grep -E "^(Authority|TeamIdentifier|Runtime)" | head -4 | tee -a "$OUT/summary.txt"
note "--- Gatekeeper"
limit 120 spctl --assess --type execute --verbose=4 "$APP" >> "$OUT/summary.txt" 2>&1 || fail "Gatekeeper rejects the app"
limit 60 xcrun stapler validate "$APP" >> "$OUT/summary.txt" 2>&1 || note "no stapled ticket"

DEST="/Applications/$(basename "$APP")"
rm -rf "$DEST"
ditto "$APP" "$DEST" || { fail "could not copy to /Applications"; exit 1; }
# No quarantine flag: on first open it would ask "downloaded from the
# Internet, open it?", which nobody can answer here. Gatekeeper's verdict
# for a download is the spctl check above.
NAME=$(defaults read "$DEST/Contents/Info.plist" CFBundleExecutable)
note "--- open $DEST"
limit 30 open "$DEST" || fail "open did not return"

for _ in $(seq 1 30); do pgrep -x "$NAME" >/dev/null && break; sleep 1; done
pgrep -x "$NAME" >/dev/null || fail "$NAME did not start"
# Let the library load, then look; the build machine may not allow screen capture.
sleep 15
limit 30 screencapture -x "$OUT/01-screen.png" 2>>"$OUT/summary.txt" || note "screen capture not allowed here"
sleep 15
if pgrep -x "$NAME" >/dev/null; then note "running after 30 s: yes"; else fail "$NAME stopped within 30 s"; fi
limit 30 screencapture -x "$OUT/02-screen.png" 2>>"$OUT/summary.txt" || true
# Anything the system logged about the app crashing.
ls ~/Library/Logs/DiagnosticReports 2>/dev/null | grep -i "$NAME" | sed 's/^/crash report: /' | tee -a "$OUT/summary.txt" || true
pkill -x "$NAME" || true
rm -rf "$WORK"
exit $failed
