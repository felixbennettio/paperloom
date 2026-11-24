#!/bin/bash
# Sourced by run.sh after android-emulator.sh. Shares a DOI (starting the
# app), an arXiv link, a publisher's page, an RIS file and a PDF.
# The share steps and their numbered artifacts stay together here.

share_text() {
  calm_system
  am_start "-a android.intent.action.SEND -t text/plain --es android.intent.extra.TEXT '$1' -n $ACTIVITY"
}

# Puts a file in Downloads and shares it through the media store, as a file
# manager would. Returns non-zero when the media store does not list it.
share_file() {
  local file=$1 type=$2 name device id uri
  calm_system
  name=$(basename "$file")
  device="/storage/emulated/0/Download/$name"
  adb push "$file" "$device" >/dev/null
  adb shell "am broadcast -a android.intent.action.MEDIA_SCANNER_SCAN_FILE -d file://$device" >/dev/null
  id=""
  for _ in $(seq 1 20); do
    id=$(adb shell "content query --uri content://media/external/file --projection _id --where \"_data='$device'\"" |
      sed -n 's/.*_id=\([0-9][0-9]*\).*/\1/p')
    id=${id%%$'\n'*}
    [ -n "$id" ] && break
    sleep 1
  done
  [ -n "$id" ] || { echo "the media store does not list $device" >&2; return 1; }
  uri="content://media/external/file/$id"
  # The address goes in as data too, so the read permission covers it.
  local intent="-a android.intent.action.SEND -t $type -d $uri --eu android.intent.extra.STREAM $uri --grant-read-uri-permission -n $ACTIVITY"
  am_start "$intent"
  if wait_shared 90; then return 0; fi
  # Only when the app was stopped: otherwise a second share would wait in
  # line behind the first and import the file twice.
  if ! running; then
    echo "    the app was stopped; sharing $name again" >&2
    am_start "$intent"
    wait_shared 90 || true
  fi
}

running() { [ -n "$(adb shell pidof "$PACKAGE" | tr -d '\r')" ]; }

# Shares text and waits until the dialog has read it; shares once more if
# the app was stopped meanwhile (see the note at the emulator's start).
# Never fails: the caller checks what the screen says.
share_text_and_wait() {
  share_text "$1"
  if wait_shared 120; then return 0; fi
  if ! running; then
    echo "    the app was stopped; sharing again" >&2
    share_text "$1"
    wait_shared 120 || true
  fi
  return 0
}

# Done, or Android Back when the dump does not show it: the app closes its
# top dialog on Back. A dialog left open would hold the next share in line.
close_dialog() {
  tap Done && return 0
  adb shell input keyevent KEYCODE_BACK
  sleep 2
}

android_share_test() {
  local APK=${1:?the APK to install}
  local failures_before=${#failed[@]}
echo "==> Install"
adb install -r -g "$APK" || { fail "installing the APK"; return 1; }

echo "==> The share sheet lists Paperloom"
for type in text/plain application/pdf application/x-research-info-systems application/x-bibtex; do
  adb shell cmd package query-activities --brief -a android.intent.action.SEND -t "$type" > "$WORK/handlers.txt"
  if grep -q "$PACKAGE" "$WORK/handlers.txt"; then
    echo "    $type: yes"
  else
    fail "share entry for $type"
  fi
done

echo "==> Open the app"
calm_system
adb shell monkey -p "$PACKAGE" -c android.intent.category.LAUNCHER 1 >/dev/null
sleep 15
shot launch
adb shell am force-stop "$PACKAGE"

echo "==> Share a DOI to the closed app"
share_text "10.1038/nature12373"
# A film of the cold start, a frame every 3 seconds, to see what the dialog
# does while the app starts.
for frame in $(seq -w 1 12); do
  sleep 3
  adb exec-out screencap -p > "$OUT/02-cold-$frame.png"
  screen_text > "$OUT/02-cold-$frame.txt"
done
wait_shared 60 || true
if on_screen "Added “"; then
  shot doi
else
  shot doi
  fail "DOI shared to the closed app"
fi
close_dialog || true

echo "==> Share an arXiv link while the app is open"
share_text_and_wait "Attention Is All You Need https://arxiv.org/abs/1706.03762"
if on_screen "Added “"; then
  shot arxiv
else
  shot arxiv
  fail "arXiv link shared to the open app"
fi
close_dialog || true

echo "==> Share a DOI while the app is open"
share_text_and_wait "10.1126/science.162.3859.1243"
if on_screen "Added “"; then
  shot doi-open
else
  shot doi-open
  fail "DOI shared to the open app"
fi
close_dialog || true

echo "==> Share a publisher's page"
share_text_and_wait "https://www.nature.com/articles/s41586-020-2649-2"
if on_screen "NumPy"; then
  shot page
  if tap "Add 1 Selected" && wait_for "Added 1" 60; then shot page-added; else warn "adding the page's work"; fi
else
  shot page
  warn "works on a publisher's page (needs nature.com)"
fi
close_dialog || true

echo "==> Share an RIS file"
printf 'TY  - JOUR\r\nTI  - Paperloom share test record\r\nAU  - Tester, Ada\r\nPY  - 2026\r\nJO  - Journal of Tests\r\nER  - \r\n' \
  > "$WORK/paperloom-share-test.ris"
if share_file "$WORK/paperloom-share-test.ris" application/x-research-info-systems; then
  if on_screen "Added 1"; then
    shot ris
  else
    shot ris
    fail "RIS file shared through the media store"
  fi
  close_dialog || true
else
  warn "RIS file: the media store did not list it"
fi

echo "==> Share a PDF"
python3 - "$WORK/paperloom-share-test.pdf" <<'EOF'
# A one-page PDF with a line of text, written by hand.
import sys

text = b"Paperloom share test PDF"
content = b"BT /F1 24 Tf 72 760 Td (" + text + b") Tj ET"
objects = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R"
    b" /Resources << /Font << /F1 5 0 R >> >> >>",
    b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
]
pdf = bytearray(b"%PDF-1.4\n")
offsets = []
for number, body in enumerate(objects, 1):
    offsets.append(len(pdf))
    pdf += b"%d 0 obj\n" % number + body + b"\nendobj\n"
xref = len(pdf)
pdf += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
for offset in offsets:
    pdf += b"%010d 00000 n \n" % offset
pdf += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, xref)
open(sys.argv[1], "wb").write(pdf)
EOF
if share_file "$WORK/paperloom-share-test.pdf" application/pdf; then
  if on_screen "PDFs added: 1"; then
    shot pdf
  else
    shot pdf
    fail "PDF shared through the media store"
  fi
  close_dialog || true
else
  warn "PDF: the media store did not list it"
fi

echo "==> Share a scanned PDF (a picture of a page, no text in it)"
# The page shows the DOI shared first: read on the phone, it makes the PDF
# join that paper; unread, the PDF stays unrecognised.
python3 -c 'import PIL' 2>/dev/null || python3 -m pip install --quiet --user pillow >/dev/null 2>&1 ||
  python3 -m pip install --quiet --user --break-system-packages pillow >/dev/null 2>&1 || true
if python3 - "$WORK/paperloom-scan-test.pdf" <<'EOF'
import sys
from PIL import Image, ImageDraw, ImageFont

page = Image.new("RGB", (1240, 1754), "white")
draw = ImageDraw.Draw(page)
try:
    font = ImageFont.truetype("DejaVuSans.ttf", 44)
except OSError:
    font = ImageFont.load_default(size=44)
for row, line in enumerate(["Nanometre-scale thermometry in a living cell", "doi:10.1038/nature12373"]):
    draw.text((100, 160 + 90 * row), line, fill="black", font=font)
page.save(sys.argv[1], "PDF", resolution=150)
EOF
then
  if share_file "$WORK/paperloom-scan-test.pdf" application/pdf; then
    wait_for "PDFs added" 120 || true
    if on_screen "Not recognised"; then
      shot scanned-pdf
      fail "scanned PDF: its DOI was not read from the picture"
    elif on_screen "already in the library: 1|PDFs added: 1"; then
      shot scanned-pdf
    else
      shot scanned-pdf
      fail "scanned PDF shared through the media store"
    fi
    close_dialog || true
  else
    warn "scanned PDF: the media store did not list it"
  fi
else
  warn "scanned PDF: no picture library (Pillow) to draw it"
fi

echo "==> The library afterwards"
sleep 3
shot library
on_screen "Nanometre-scale thermometry" || warn "the DOI's work is not in the visible list"

  if [ ${#failed[@]} -eq "$failures_before" ]; then echo "All share steps passed."; fi
  return 0
}
