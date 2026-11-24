#!/bin/bash
# Sourced by run.sh: one emulator and one set of artifacts for both tests.
# Nothing starts until android_test_setup and android_start_emulator are called.

PACKAGE=io.felixbennett.paperloom
ACTIVITY="$PACKAGE/.MainActivity"
SYSTEM_IMAGE="system-images;android-34;google_apis;x86_64"

fail() { echo "FAILED: $*" >&2; failed+=("$*"); }
warn() { echo "WARNING: $*" >&2; warned+=("$*"); }

# Whatever happens, keep both parts' results and clean up our temporary files.
keep_logs() {
  local status=$?
  trap - EXIT
  # Bounded: without a device adb waits for one.
  timeout 60 adb logcat -d > "$OUT/logcat.txt" 2>/dev/null || true
  {
    echo "exit status: $status"
    for f in "${failed[@]}"; do echo "failed: $f"; done
    for w in "${warned[@]}"; do echo "warning: $w"; done
  } > "$OUT/summary.txt"
  rm -rf "$WORK"
}

android_test_setup() {
  OUT="$ROOT/test-results/android"
  rm -rf "$OUT" && mkdir -p "$OUT" || return 1
  WORK=$(mktemp -d) || return 1
  failed=()
  warned=()
  step=0
  trap keep_logs EXIT
  : "${ANDROID_HOME:=${ANDROID_SDK_ROOT:?no Android SDK (ANDROID_HOME)}}"
  export PATH="$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
cat > "$WORK/screen.py" <<'EOF'
# Legacy text/tap commands also serve the share test.
import re
import sys
import xml.etree.ElementTree as ET

mode, path = sys.argv[1:3]
args = sys.argv[3:]
try:
    root = ET.parse(path).getroot()
except (OSError, ET.ParseError):
    sys.exit(2 if mode.startswith("phone-") else 0)
nodes = list(root.iter("node"))


def labels(node):
    return [(node.get(k) or "").strip() for k in ("text", "content-desc", "hint")]


def bounds(node):
    values = re.findall(r"-?\d+", node.get("bounds", ""))
    return tuple(map(int, values)) if len(values) == 4 else (0, 0, 0, 0)


def centre(node):
    x1, y1, x2, y2 = bounds(node)
    print((x1 + x2) // 2, (y1 + y2) // 2)


if mode == "text":
    for node in nodes:
        for value in labels(node)[:2]:
            if value:
                print(value)
    sys.exit(0)
if mode == "tap":
    for node in nodes:
        if args[0] in labels(node)[:2]:
            centre(node)
            break
    sys.exit(0)

# hierarchy normally has no bounds; its first top-level node is the screen.
frame = root if root.get("bounds") else root.find("node")
width, height = bounds(frame)[2:] if frame is not None else (0, 0)
parents = {child: parent for parent in root.iter() for child in parent}
web = set()
for node in nodes:
    if node.get("class") == "android.webkit.WebView" and node.get("package") == "io.felixbennett.paperloom":
        web.update(node.iter("node"))
if not web or not width or not height:
    sys.exit(2)


def visible(node):
    x1, y1, x2, y2 = bounds(node)
    return x2 > x1 and y2 > y1 and x1 < width and x2 > 0 and y1 < height and y2 > 0


def button(node):
    return node.get("enabled", "true") == "true" and (
        node.get("clickable") == "true" or node.get("class") == "android.widget.Button")


shown = [n for n in nodes if n in web and visible(n)]
def nav(label):
    matches = [n for n in shown if label in labels(n) and button(n) and bounds(n)[1] > height * 0.65]
    return max(matches, key=lambda n: bounds(n)[1]) if matches else None


mode = mode.removeprefix("phone-")
if mode == "text":
    for node in shown:
        for value in labels(node)[:2]:
            if value:
                print(value)
    sys.exit(0)
if mode == "ready":
    # Do not require the bottom bar here: its disappearance must still fail
    # the explicit navigation check when the rest of the page is readable.
    words = {value for node in shown for value in labels(node) if value}
    sys.exit(0 if len(shown) >= 12 and len(words) >= 5 else 2)
if mode == "nav":
    found = nav(args[0])
elif mode == "has":
    found = next((n for n in shown if args[0] in labels(n)), None)
elif mode == "field":
    fields = [n for n in shown if n.get("class") == "android.widget.EditText" and n.get("enabled", "true") == "true"]
    # The WebView names a field by its hint, which uiautomator's dump leaves
    # out: the page's only field is the one meant.
    found = next((n for n in fields if not args or args[0] in labels(n)), None) or (
        fields[0] if len(fields) == 1 else None)
elif mode == "value":
    found = next((n for n in shown if n.get("class") == "android.widget.EditText"
                  and (n.get("text") or "").strip() == args[0]), None)
elif mode == "tap":
    matches = [n for n in shown if args[0] in labels(n) and button(n)]
    found = (matches[-1] if len(args) > 1 and args[1] == "last" else matches[0]) if matches else None
elif mode == "back":
    # phone-back shows the previous page's title, not a literal "Back".
    # From a fresh Library it is "All Items"; the bottom bar is excluded.
    found = next((n for n in shown if button(n) and "All Items" in labels(n)
                  and bounds(n)[1] < height * 0.5), None)
elif mode in ("item", "word"):
    # The first row's star locates the title button beside it. Do not tap the
    # star itself or guess a coordinate when the WebView omits a row.
    found = None
    stars = sorted((n for n in shown if any(v in labels(n) for v in ("Add Star", "Remove Star"))),
                   key=lambda n: bounds(n)[1])
    for star in stars:
        sx1, sy1, sx2, sy2 = bounds(star)
        candidates = [n for n in shown if button(n) and any(labels(n))
                      and bounds(n)[0] >= sx2 - 8
                      and sy1 <= (bounds(n)[1] + bounds(n)[3]) // 2 <= sy2]
        if candidates:
            found = min(candidates, key=lambda n: bounds(n)[0])
            break
elif mode == "overflow":
    print(f"screen width: {width}px")
    for node in nodes:
        if node not in web:
            continue
        x1, y1, x2, y2 = bounds(node)
        if x2 <= x1 or y2 <= y1 or not (x1 > width or x2 > width + 8):
            continue
        parent = node
        sideways = False
        while parent is not None and parent in web:
            # SearchView's Database radiogroup is the source-chip row.
            # A generic scrollable WebView may scroll vertically; it does
            # not excuse horizontal overflow throughout the whole page.
            if "Database" in labels(parent) or parent.get("class") == "android.widget.HorizontalScrollView":
                sideways = True
            parent = parents.get(parent)
        kind = "sideways row (allowed)" if sideways else "possible layout overflow"
        text = next((v for v in labels(node) if v), "(no text)")
        print(f"{kind}: {node.get('bounds')} {node.get('class', '')} {text}")
    sys.exit(0)
else:
    raise ValueError(mode)
if found is None:
    sys.exit(1)
if mode == "word":
    words = re.findall(r"[A-Za-z]{3,}", " ".join(labels(found)))
    if not words:
        sys.exit(1)
    print(words[0])
    sys.exit(0)
centre(found)
EOF

}

dump() {
  rm -f "$WORK/ui.xml"
  adb shell rm -f /sdcard/ui.xml >/dev/null 2>&1 || true
  adb shell uiautomator dump /sdcard/ui.xml >/dev/null 2>&1 || true
  adb pull /sdcard/ui.xml "$WORK/ui.xml" >/dev/null 2>&1 || : > "$WORK/ui.xml"
}
screen_text() { dump; python3 "$WORK/screen.py" text "$WORK/ui.xml"; }

# A slow emulator sometimes says the System UI is not responding; that
# dialog covers everything, so wait it out rather than fail the step.
calm_system() {
  local at
  for _ in 1 2 3; do
    screen_text > "$WORK/system.txt"
    grep -q "isn't responding" "$WORK/system.txt" || return 0
    at=$(python3 "$WORK/screen.py" tap "$WORK/ui.xml" Wait)
    # shellcheck disable=SC2086
    if [ -n "$at" ]; then adb shell input tap $at; fi
    sleep 5
  done
}
# Whether the screen shows text matching the regular expression $1. (Through
# a file: `grep -q` in a pipe would cut the writer off, which pipefail counts
# as a failure.)
# uiautomator often leaves out parts of the WebView in one dump and has them
# in the next, so look three times before saying no.
on_screen() {
  local try
  for try in 1 2 3; do
    screen_text > "$WORK/screen.txt"
    grep -Eq -- "$1" "$WORK/screen.txt" && return 0
    [ "$try" = 3 ] || sleep 1
  done
  return 1
}

# Waits up to $2 seconds until the screen shows text matching the regular
# expression $1.
wait_for() {
  local until=$((SECONDS + $2))
  while [ $SECONDS -lt $until ]; do
    if on_screen "$1"; then return 0; fi
    sleep 2
  done
  return 1
}

# Waits up to $1 seconds until the share dialog is up and done reading.
wait_shared() {
  local until=$((SECONDS + $1))
  while [ $SECONDS -lt $until ]; do
    if on_screen "Shared to Paperloom" && ! grep -q "Reading what was shared" "$WORK/screen.txt"; then return 0; fi
    sleep 2
  done
  return 1
}

# Taps the element labelled exactly $1 (looking three times, as on_screen).
tap() {
  local at="" try
  for try in 1 2 3; do
    dump
    at=$(python3 "$WORK/screen.py" tap "$WORK/ui.xml" "$1")
    [ -n "$at" ] && break
    [ "$try" = 3 ] || sleep 1
  done
  if [ -z "$at" ]; then echo "nothing labelled \"$1\" on screen" >&2; return 1; fi
  # shellcheck disable=SC2086
  adb shell input tap $at
  sleep 2
}

step=0
# Keeps a screenshot and the screen's text as NN-$1.
shot() {
  step=$((step + 1))
  local name
  name=$(printf '%02d-%s' "$step" "$1")
  adb exec-out screencap -p > "$OUT/$name.png" || fail "screenshot $name"
  screen_text > "$OUT/$name.txt"
  echo "--- $name"
  sed 's/^/    /' "$OUT/$name.txt"
}

# What each `am start` answered, for when a share did not arrive.
am_start() {
  echo "--- am start $*" >> "$OUT/am.txt"
  adb shell "am start -W $*" >> "$OUT/am.txt" 2>&1
}


android_start_emulator() {
echo "==> Emulator"
{ yes || true; } | sdkmanager --install "$SYSTEM_IMAGE" emulator platform-tools >/dev/null || return 1
export PATH="$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$PATH"
# One place for the AVD: on Azure avdmanager and the emulator looked in
# different ones ("Unknown AVD name").
export ANDROID_AVD_HOME="$HOME/.android/avd"
mkdir -p "$ANDROID_AVD_HOME"
echo no | avdmanager create avd --force -n share -k "$SYSTEM_IMAGE" -d pixel_6 >/dev/null || return 1
if [ ! -f "$ANDROID_AVD_HOME/share.ini" ]; then
  local made
  made=$(find "$HOME" /usr/local/lib/android -name share.ini -path '*avd*' 2>/dev/null | head -1)
  echo "the AVD was made in ${made:-no place found}"
  [ -n "$made" ] || return 1
  export ANDROID_AVD_HOME=${made%/*}
fi
# Azure's agents have KVM, for the kvm group only.
if [ -e /dev/kvm ] && [ ! -w /dev/kvm ]; then sudo chmod 666 /dev/kvm || true; fi
# The APK is built by now, so the emulator can have the machine: CircleCI's
# large has 4 cores and 15 GB, Azure's agents 2 and 7 (2.5 GB stay free).
local cores memory emulator
cores=$(nproc)
[ "$cores" -le 4 ] || cores=4
memory=$(($(awk '/^MemTotal/ { print int($2 / 1024) }' /proc/meminfo) - 2560))
[ "$memory" -le 6144 ] || memory=6144
[ "$memory" -ge 2048 ] || memory=2048
echo "emulator: $cores cores, $memory MB"
emulator -avd share -no-window -no-audio -no-boot-anim -no-snapshot -gpu swiftshader_indirect -cores "$cores" -memory "$memory" \
  > "$OUT/emulator.log" 2>&1 &
emulator=$!
booting=$SECONDS
until [ "$(timeout 20 adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do
  if ! kill -0 "$emulator" 2>/dev/null; then echo "the emulator stopped" >&2; tail -50 "$OUT/emulator.log" >&2; return 1; fi
  if [ $((SECONDS - booting)) -gt 600 ]; then echo "the emulator did not boot" >&2; tail -50 "$OUT/emulator.log" >&2; return 1; fi
  sleep 3
done
# Keep the phone viewport fixed, even if the device profile changes.
adb shell wm size 1080x2400 || return 1
adb shell wm density 420 || return 1
for setting in window_animation_scale transition_animation_scale animator_duration_scale; do
  adb shell settings put global "$setting" 0
done
# No "isn't responding" or crash dialogs over the app (a crash still ends
# the app, and the steps notice).
adb shell settings put global hide_error_dialogs 1
# Keep the whole test in the device log, not just its last minutes.
adb logcat -G 16M >/dev/null 2>&1 || true
adb shell svc power stayon true
adb shell wm dismiss-keyguard || true
adb shell input keyevent KEYCODE_WAKEUP
# A fresh emulator restarts Google Play services shortly after booting, and
# Android stops apps that use its providers (the WebView's fonts) with it.
sleep 60

}

android_finish() {
  [ ${#warned[@]} -eq 0 ] || echo "Warnings: ${warned[*]}" >&2
  if [ ${#failed[@]} -gt 0 ]; then
    echo "Failed: ${failed[*]}" >&2
    return 1
  fi
  echo "All Android steps passed."
}
