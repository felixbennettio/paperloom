#!/bin/bash
# The Linux app as a person installs it (.deb), in a virtual display, driven
# through tauri-driver and WebKitWebDriver by desktop-e2e.mjs, with a fresh
# home of its own, Tesseract for scanned pages and the real network. Results
# go to test-results/desktop-e2e (screenshots, page text, summary.txt and
# the app's output in driver.log). run.sh desktop-e2e calls it with
# DESKTOP_APP, a folder holding the .deb (Azure: an earlier job's).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=$ROOT/test-results/desktop-e2e
rm -rf "$OUT" && mkdir -p "$OUT"
FILES=$(mktemp -d)
APP_HOME=$(mktemp -d)
TOOLS=$(mktemp -d)

deb=$(find "${DESKTOP_APP:?a folder with the .deb}" -name '*.deb' | head -1)
[ -n "$deb" ] || { echo "no .deb in $DESKTOP_APP" >&2; exit 1; }
sudo=""
if [ "$(id -u)" != 0 ]; then sudo=sudo; fi
export DEBIAN_FRONTEND=noninteractive
$sudo apt-get update -qq
$sudo apt-get install -y -qq --no-install-recommends "$deb" xvfb xdotool dbus-x11 webkit2gtk-driver \
  tesseract-ocr tesseract-ocr-eng tesseract-ocr-chi-sim python3-pil fonts-dejavu-core fonts-wqy-microhei imagemagick >/dev/null
package=$(dpkg-deb -f "$deb" Package)
app=$(dpkg -L "$package" | grep -m1 '^/usr/bin/')
echo "installed $package: $app"

# tauri-driver passes WebDriver on to WebKitWebDriver and starts the app.
cargo install tauri-driver --version "${TAURI_DRIVER_VERSION:-2.1.0}" --locked --root "$TOOLS" --quiet

python3 "$ROOT/scripts/ci/e2e-files.py" "$FILES"

Xvfb :99 -screen 0 1440x900x24 -nolisten tcp > "$OUT/xvfb.log" 2>&1 &
xvfb=$!
export DISPLAY=:99
eval "$(dbus-launch --sh-syntax)"
# The app's own home, so it starts as on a new computer.
env -u XDG_DATA_HOME -u XDG_CONFIG_HOME -u XDG_CACHE_HOME HOME="$APP_HOME" TAURI_WEBVIEW_AUTOMATION=true \
  "$TOOLS/bin/tauri-driver" --port 4444 > "$OUT/driver.log" 2>&1 &
driver=$!
cleanup() {
  kill "$driver" "$xvfb" 2>/dev/null || true
  kill "${DBUS_SESSION_BUS_PID:-}" 2>/dev/null || true
  rm -rf "$TOOLS"
}
trap cleanup EXIT
for _ in $(seq 1 30); do
  curl -sf http://127.0.0.1:4444/status >/dev/null && break
  sleep 1
done

status=0
APP_HOME=$APP_HOME node "$ROOT/scripts/ci/desktop-e2e.mjs" "$app" "$OUT" "$FILES" || status=$?
cp "$FILES"/*.docx "$OUT/" 2>/dev/null || true
exit "$status"
