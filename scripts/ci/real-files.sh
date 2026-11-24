#!/bin/bash
# Real programs on real files, on Linux: Tesseract reads a drawn picture
# through the app's OCR crate, and LibreOffice opens the Word and OpenDocument
# files the core cited. run.sh core calls it when CI_REAL_FILES is set.
#
#   scripts/ci/real-files.sh prepare   installs Tesseract, LibreOffice Writer
#                                      and a Chinese font; draws $REAL_FILES/ocr.png
#   scripts/ci/real-files.sh check     LibreOffice opens the cited copies the
#                                      core's tests left in $REAL_FILES/office
set -euo pipefail

OUT=${REAL_FILES:-/tmp/paperloom-real}
FONT=/usr/share/fonts/truetype/wqy/wqy-microhei.ttc

prepare() {
  local sudo=""
  if [ "$(id -u)" != 0 ]; then sudo=sudo; fi
  export DEBIAN_FRONTEND=noninteractive
  # linux-deps.sh ran apt-get update already.
  $sudo apt-get install -y -qq --no-install-recommends \
    tesseract-ocr tesseract-ocr-eng tesseract-ocr-chi-sim fonts-wqy-microhei \
    libreoffice-writer-nogui python3-pil >/dev/null
  tesseract --version | head -1
  soffice --version
  mkdir -p "$OUT/office"
  # Part of a page as the app draws it for recognition (A4 at 200 dpi, 1654
  # wide; a 13-point line is 36 pixels): black type on white.
  python3 - "$OUT/ocr.png" "$FONT" <<'PYEOF'
import sys
from PIL import Image, ImageDraw, ImageFont

path, font = sys.argv[1:3]
image = Image.new("L", (1654, 300), 255)
draw = ImageDraw.Draw(image)
type_ = ImageFont.truetype(font, 36)
draw.text((160, 80), "Thermometry with nanodiamonds", font=type_, fill=0)
draw.text((160, 170), "纳米金刚石量子测温", font=type_, fill=0)
image.save(path)
PYEOF
  echo "drew $OUT/ocr.png"
  # What Tesseract makes of it with each set of languages, for the log.
  echo "languages: $(tesseract --list-langs 2>&1 | tail -n +2 | tr '\n' ' ')"
  local langs
  for langs in chi_sim eng; do
    echo "tesseract -l $langs: $(tesseract "$OUT/ocr.png" stdout -l "$langs" 2>/dev/null | tr -s '\n' '|')"
  done
}

check() {
  local office=$OUT/office kind
  for kind in docx odt; do
    [ -f "$office/cited-paper.$kind" ] || { echo "no cited-paper.$kind: the core's tests did not write it" >&2; return 1; }
    # Each into its own folder: both come out as cited-paper.html. A profile
    # of its own (beside the files kept), so a first start does not depend on
    # the runner's home.
    soffice -env:UserInstallation="file://$OUT.profile" --headless \
      --convert-to html --outdir "$OUT/html/$kind" "$office/cited-paper.$kind"
  done
  # Footnotes are part of Writer's HTML (at the end), so the note's citation
  # is checked too.
  python3 - "$OUT/html" <<'PYEOF'
import html, pathlib, re, sys

expected = [
    "Temperature can be measured inside living cells",
    "[1]",
    "Nanometre-scale thermometry in a living cell",
    "Nature",
    "Radioactivity",
]
problems = []
for kind in ("docx", "odt"):
    page = pathlib.Path(sys.argv[1], kind, "cited-paper.html")
    if not page.is_file():
        problems.append(f"{kind}: LibreOffice did not open it")
        continue
    # Blocks end with a space; inline tags join (a number may be in runs of its own).
    text = re.sub(r"<(style|title).*?</\1>", "", page.read_text(errors="replace"), flags=re.S | re.I)
    text = re.sub(r"</?(p|div|br|li|h\d|td|tr)\b[^>]*>", " ", text, flags=re.I)
    text = re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", "", text)))
    print(f"{kind}: {text.strip()[:600]}")
    problems += [f"{kind}: no {want!r}" for want in expected if want.lower() not in text.lower()]
    problems += [f"{kind}: still has {left!r}" for left in ("[@", "{bibliography}", "{参考文献}") if left in text]
for problem in problems:
    print(problem, file=sys.stderr)
sys.exit(1 if problems else 0)
PYEOF
}

case "${1:-}" in
  prepare) prepare ;;
  check) check ;;
  *) echo "usage: $0 prepare|check" >&2; exit 2 ;;
esac
