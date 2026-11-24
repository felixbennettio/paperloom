"""The files the desktop end-to-end test adds to the app, written into a folder.

  python3 scripts/ci/e2e-files.py <folder>

scan.pdf is a scanned page: no text in the PDF, only its picture. Its DOI
joins it to the paper the test adds by DOI; its last line holds the word the
test searches for. paper.docx is a Word document citing that paper by its DOI,
with the reference list's place marked. curie.ris is opened with the app as a
file manager's Open With does. Needs Pillow.
"""
import sys
import zipfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

folder = Path(sys.argv[1])
folder.mkdir(parents=True, exist_ok=True)


def font(size):
    # DejaVu on Linux, Arial on Windows.
    for name in ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "DejaVuSans.ttf", "arial.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            pass
    return ImageFont.load_default(size=size)


page = Image.new("RGB", (1240, 1754), "white")
draw = ImageDraw.Draw(page)
lines = ["Nanometre-scale thermometry in a living cell", "doi:10.1038/nature12373", "", "Notes from the lighthouse keeper"]
for row, line in enumerate(lines):
    draw.text((100, 160 + 80 * row), line, fill="black", font=font(40))
page.save(folder / "scan.pdf", "PDF", resolution=150)


def paragraph(text):
    return f'<w:p><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>'


body = paragraph("Temperature can be measured inside living cells [@10.1038/nature12373].") + paragraph("{bibliography}")
parts = {
    "[Content_Types].xml": '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
    "_rels/.rels": '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
    "word/document.xml": f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr/></w:body></w:document>',
}
with zipfile.ZipFile(folder / "paper.docx", "w", zipfile.ZIP_DEFLATED) as docx:
    for name, text in parts.items():
        docx.writestr(name, text)
# A reference file to open with Paperloom, as from a file manager.
(folder / "curie.ris").write_text(
    "TY  - JOUR\nTI  - Radioactivity\nAU  - Curie, Marie\nPY  - 1903\nJO  - Comptes rendus\nER  - \n",
    encoding="utf-8",
)
print(f"wrote scan.pdf, paper.docx and curie.ris in {folder}")
