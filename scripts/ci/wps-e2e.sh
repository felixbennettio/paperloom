#!/bin/bash
# WPS for Linux in a virtual display with the probe add-in (wps-probe/),
# which reports what WPS's JS add-in runtime offers the Paperloom add-in:
# whether add-ins load from the jsaddons folder (online and offline), the
# document operations (content controls, bookmarks, fields, variables,
# formatting), the task pane and requests to 127.0.0.1. The first step of
# the WPS add-in (docs/writing-plugins.md). Results go to
# test-results/wps-e2e: reports.jsonl, requests.log, screenshots, WPS's
# output and the saved document's XML.
#
# With WPS_APP (a folder with Paperloom's .deb, as linux-app leaves in dist/)
# it tests the real add-in instead (WPS_ADDIN_BUILD: an add-in build to put
# in place of the package's): the app, started through tauri-driver,
# installs it from Settings, and WPS cites a paper of the app's library
# through its task pane (desktop-e2e.mjs, steps "Install the WPS add-in" and
# "Cite in WPS"); the saved document must hold the fields.
#
# WPS_URL: the .deb (default: the China personal build, from linux.wps.cn,
# whose page signs its links with md5(key + path + time); this does the
# same); WPS_SUM: sha256:…, md5:… or size:… of it.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=$ROOT/test-results/wps-e2e
PROBE=$ROOT/scripts/ci/wps-probe
rm -rf "$OUT" && mkdir -p "$OUT"
WORK=$(mktemp -d)
WPS_URL=${WPS_URL:-https://wps-linux-personal.wpscdn.cn/wps/download/ep/Linux2023/29183/wps-office_12.1.2.29183.AK.preread.sw.Personal_806791_amd64.deb}
WPS_SUM=${WPS_SUM:-sha256:77ffbdc1be76c834e1853c27ff9063e49d5d3b1f30b206d785761b5bf560c83b}

sudo=""
if [ "$(id -u)" != 0 ]; then sudo=sudo; fi
python3 - "$WPS_URL" "$WORK/wps.deb" <<'EOF'
import hashlib, subprocess, sys, time
from urllib.parse import urlparse
url, out = sys.argv[1:]
if urlparse(url).hostname.endswith(".wpscdn.cn"):
    t = int(time.time())
    key = hashlib.md5(f"7f8faaaa468174dc1c9cd62e5f218a5b{urlparse(url).path}{t}".encode()).hexdigest()
    url = f"{url}?t={t}&k={key}"
subprocess.run(["curl", "-fsSL", "--retry", "3", "-o", out, url], check=True)
EOF
echo "downloaded $(stat -c %s "$WORK/wps.deb") bytes, sha256 $(sha256sum "$WORK/wps.deb" | cut -d' ' -f1)"
kind=${WPS_SUM%%:*}
if [ "$kind" = size ]; then
  [ "$(stat -c %s "$WORK/wps.deb")" = "${WPS_SUM#*:}" ]
else
  echo "${WPS_SUM#*:}  $WORK/wps.deb" | "${kind}sum" -c -
fi

app_packages=()
if [ -n "${WPS_APP:-}" ]; then
  deb=$(find "$WPS_APP" -name '*.deb' | head -1)
  [ -n "$deb" ] || { echo "no .deb in $WPS_APP" >&2; exit 1; }
  app_packages=("$deb" webkit2gtk-driver)
fi
export DEBIAN_FRONTEND=noninteractive
$sudo apt-get update -qq
$sudo apt-get install -y -qq --no-install-recommends "$WORK/wps.deb" "${app_packages[@]}" xvfb xdotool dbus-x11 \
  imagemagick python3-pil fonts-dejavu-core fonts-wqy-microhei unzip >/dev/null
dpkg -s wps-office | grep -E '^(Version|Depends):' | tee "$OUT/package.txt"
OEM=/opt/kingsoft/wps-office/office6/cfgs/oem.ini
cp "$OEM" "$OUT/oem.ini.orig" 2>/dev/null || true

# tauri-driver passes WebDriver on to WebKitWebDriver and starts the app;
# installed before HOME changes, with the runner's Rust toolchain.
if [ -n "${WPS_APP:-}" ]; then
  TOOLS=$(mktemp -d)
  cargo install tauri-driver --version "${TAURI_DRIVER_VERSION:-2.1.0}" --locked --root "$TOOLS" --quiet
fi

# WPS in a home of its own, its licence agreement accepted.
export HOME=$WORK/home
unset XDG_DATA_HOME XDG_CONFIG_HOME XDG_CACHE_HOME
mkdir -p "$HOME/.config/Kingsoft"
printf '[6.0]\ncommon\\AcceptedEULA=true\n' > "$HOME/.config/Kingsoft/Office.conf"
ADDONS=$HOME/.local/share/Kingsoft/wps/jsaddons
python3 "$ROOT/scripts/ci/e2e-files.py" "$WORK/files" >/dev/null
python3 "$PROBE/server.py" "$OUT" > "$OUT/server.log" 2>&1 &
server=$!
Xvfb :99 -screen 0 1440x900x24 -nolisten tcp > "$OUT/xvfb.log" 2>&1 &
xvfb=$!
export DISPLAY=:99
eval "$(dbus-launch --sh-syntax)"
stop_wps() {
  pkill -f /opt/kingsoft/wps-office 2>/dev/null || true
  sleep 2
  pkill -9 -f /opt/kingsoft/wps-office 2>/dev/null || true
}
cleanup() {
  stop_wps
  kill "${driver:-}" "$server" "$xvfb" 2>/dev/null || true
  kill "${DBUS_SESSION_BUS_PID:-}" 2>/dev/null || true
}
trap cleanup EXIT
sleep 2

if [ -n "${WPS_APP:-}" ]; then
  package=$(dpkg-deb -f "$deb" Package)
  app=$(dpkg -L "$package" | grep -m1 '^/usr/bin/')
  echo "installed $package: $app"
  if [ -n "${WPS_ADDIN_BUILD:-}" ]; then
    # This commit's add-in in place of the package's.
    resource=$(dirname "$(dpkg -L "$package" | grep -m1 '/wps-addin/ribbon.xml$')")
    $sudo rm -rf "$resource" && $sudo cp -r "$WPS_ADDIN_BUILD" "$resource"
    echo "add-in from this commit in $resource"
  fi
  TAURI_WEBVIEW_AUTOMATION=true "$TOOLS/bin/tauri-driver" --port 4444 > "$OUT/driver.log" 2>&1 &
  driver=$!
  for _ in $(seq 1 30); do
    curl -sf http://127.0.0.1:4444/status >/dev/null && break
    sleep 1
  done
  cp "$WORK/files/paper.docx" "$OUT/wps-paper.docx"
  status=0
  APP_HOME=$HOME WPS_DOCUMENT=$OUT/wps-paper.docx WPS_REPORTS=$OUT/reports.jsonl \
    E2E_STEPS="Start,Add by DOI,Open a reference file with Paperloom,Install the WPS add-in,Cite in WPS" \
    node "$ROOT/scripts/ci/desktop-e2e.mjs" "$app" "$OUT" "$WORK/files" || status=$?
  for part in word/document.xml word/settings.xml; do
    unzip -p "$OUT/wps-paper.docx" "$part" > "$OUT/wps-paper-$(basename "$part")" 2>/dev/null || true
  done
  cp -r "$ADDONS" "$OUT/jsaddons" 2>/dev/null || true
  exit "$status"
fi

# The probe add-in, online and offline.
mkdir -p "$ADDONS/paperloomoffline_1.0.0"
cp "$PROBE"/{ribbon.xml,index.html,main.js,taskpane.html} "$ADDONS/paperloomoffline_1.0.0/"
# Both lists: jsplugins.xml as wpsjs debug writes it, publish.xml as its
# install page does.
cat > "$ADDONS/jsplugins.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<jsplugins>
  <jspluginonline name="paperloomonline" type="wps" url="http://127.0.0.1:8977/addin/" debug="" enable="enable_dev" install="null"/>
</jsplugins>
EOF
cat > "$ADDONS/publish.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<jsplugins>
  <jsplugin name="paperloomoffline" type="wps" url="http://127.0.0.1:8977/paperloomoffline.7z" version="1.0.0" enable="enable" install="null"/>
</jsplugins>
EOF

shot() {
  import -window root "$OUT/$1.png" 2>/dev/null || true
  for w in $(xdotool search --onlyvisible --name '.' 2>/dev/null); do
    echo "$1 window $w: $(xdotool getwindowname "$w" 2>/dev/null)"
  done >> "$OUT/windows.txt"
}

reported() { grep -q "\"step\": \"$1\"" "$OUT/reports.jsonl" 2>/dev/null; }

# What WPS asks at its first start, in a 1440x900 screen: "WPS Office is not
# your default office software" (OK), then for each add-in whether it may
# run (Allow) and whether to trust it (OK). Clicked where they show, twice
# for the two add-ins; the screenshots show them.
answer() {
  for w in $(xdotool search --onlyvisible --name '.' 2>/dev/null); do
    echo "$1 window $w: $(xdotool getwindowname "$w" 2>/dev/null) $(xdotool getwindowgeometry "$w" 2>/dev/null | tr '\n' ' ')"
  done >> "$OUT/windows.txt"
  xdotool mousemove 937 510 click 1
  sleep 2
  shot "$1-default-answered"
  for round in 1 2; do
    xdotool mousemove 748 596 click 1
    sleep 3
    shot "$1-addin-allowed-$round"
    xdotool mousemove 776 481 click 1
    sleep 4
    shot "$1-addin-trusted-$round"
  done
}

# One start of WPS: until the probe is done, or 90 s without the add-in.
run() {
  local name=$1
  cp "$WORK/files/paper.docx" "$OUT/$name.docx"
  wps "$OUT/$name.docx" > "$OUT/$name.log" 2>&1 &
  local waited=0
  while [ $waited -lt 180 ]; do
    sleep 5
    waited=$((waited + 5))
    case $waited in 15|45) shot "$name-${waited}s" ;; 20) answer "$name" ;; esac
    reported done && break
    [ $waited -ge 90 ] && ! reported OnAddinLoad && break
  done
  sleep 3
  shot "$name-end"
  stop_wps
}

oem() {
  # Sets key=value in a section of WPS's oem.ini.
  $sudo python3 - "$OEM" "$@" <<'EOF'
import configparser, sys
path, section, *pairs = sys.argv[1:]
ini = configparser.RawConfigParser(strict=False)
ini.optionxform = str
with open(path, encoding="utf-8", errors="surrogateescape") as f:
    ini.read_file(f)
if not ini.has_section(section):
    ini.add_section(section)
for pair in pairs:
    key, value = pair.split("=", 1)
    ini.set(section, key, value)
with open(path, "w", encoding="utf-8", errors="surrogateescape") as f:
    ini.write(f, space_around_delimiters=False)
EOF
}

run plain
if ! reported OnAddinLoad; then
  echo "no add-in loaded as installed; with JsApiPlugin on"
  oem Support JsApiPlugin=true JsApiShowWebDebugger=true
  run jsapi
fi
if ! reported OnAddinLoad; then
  echo "no add-in loaded; with a JS plugins server"
  oem Server JSPluginsServer=http://127.0.0.1:8977/jsplugins.xml
  run server
fi

cp "$OEM" "$OUT/oem.ini" 2>/dev/null || true
cp -r "$ADDONS" "$OUT/jsaddons" 2>/dev/null || true
# Where WPS keeps the answer to "may this add-in use the document".
grep -rl -i "8977\|paperloom" "$HOME" 2>/dev/null | grep -v '/jsaddons/paperloomoffline' > "$OUT/answer-files.txt" || true
mkdir -p "$OUT/answer"
while read -r f; do cp "$f" "$OUT/answer/$(echo "${f#"$HOME"/}" | tr / _)" 2>/dev/null || true; done < "$OUT/answer-files.txt"
cp "$HOME/.config/Kingsoft/Office.conf" "$OUT/Office.conf" 2>/dev/null || true
find "$HOME" -type f \( -name '*.log' -o -name '*.conf' -o -name '*.xml' \) -size -2M 2>/dev/null \
  | grep -v '/jsaddons/' | head -50 > "$OUT/home-files.txt" || true
for doc in "$OUT"/*.docx; do
  for part in word/document.xml word/settings.xml; do
    unzip -p "$doc" "$part" > "${doc%.docx}-$(basename "$part")" 2>/dev/null || true
  done
done
python3 - "$OUT" <<'EOF'
import json, re, sys
from pathlib import Path
out = Path(sys.argv[1])
lines = (out / "reports.jsonl").read_text(encoding="utf-8").splitlines() if (out / "reports.jsonl").exists() else []
summary = []
for line in lines:
    r = json.loads(line)
    summary.append(f"{r.get('source')} {r.get('step')}: {'ok' if r.get('ok') == '1' else 'FAILED'} {r.get('detail', '')[:300]}")
for xml in sorted(out.glob("*-document.xml")):
    text = xml.read_text(encoding="utf-8", errors="replace")
    counts = {tag: len(re.findall(f"<w:{tag}[ >]", text)) for tag in ("sdt", "bookmarkStart", "fldSimple", "instrText", "vertAlign", "i")}
    tags = re.findall(r'paperloom[^"<]*', text)[:6]
    summary.append(f"{xml.name}: {counts}; paperloom tags: {tags}")
for xml in sorted(out.glob("*-settings.xml")):
    text = xml.read_text(encoding="utf-8", errors="replace")
    summary.append(f"{xml.name}: docVars {re.findall(r'<w:docVar [^>]*>', text)[:4]}")
(out / "summary.txt").write_text("\n".join(summary) + "\n", encoding="utf-8")
print("\n".join(summary) or "no reports")
EOF
reported OnAddinLoad 2>/dev/null
