#!/usr/bin/env python3
"""Puts the browser extension together in target/extension (load it
unpacked from there) and zips it as target/paperloom-extension.zip:

    extension/                      manifest, popup
    core/crates/capture/src/extract.js + core/rules/sites.json
                                    -> capture.js, the same script the
                                       in-app browser runs
    i18n/strings.json               -> _locales/<language>/messages.json

    python3 scripts/build-extension.py
"""

import json
import re
import shutil
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "extension"
OUT = ROOT / "target" / "extension"
ZIP = ROOT / "target" / "paperloom-extension.zip"

# The extension's messages, by the English text in i18n/strings.json.
MESSAGES = {
    "name": "Paperloom",
    "description": "Save the papers on the page you are reading to Paperloom.",
    "save": "Save to Paperloom",
    "already": "Already in your library",
    "saved": "Works added: {added}; already in the library: {duplicates}.",
    "noApp": "Open Paperloom on this computer first.",
    "reading": "Reading this page…",
    "found": "Works on this page: {count}.",
    "nothing": "Nothing to save on this page.",
    "cannotRead": "This page cannot be read.",
}

# The browser's names for the languages the apps speak.
LOCALES = {"en": None, "zh_CN": "zh-Hans", "zh_TW": "zh-Hant", "fr": "fr", "ja": "ja", "ru": "ru"}
PLACEHOLDER = re.compile(r"\{(\w+)\}")


def numbered(text):
    """`{added}` and `{duplicates}` become $1 and $2, in order."""
    names = []

    def number(match):
        if match.group(1) not in names:
            names.append(match.group(1))
        return f"${names.index(match.group(1)) + 1}"

    return PLACEHOLDER.sub(number, text)


def main():
    strings = json.loads((ROOT / "i18n" / "strings.json").read_text(encoding="utf-8"))["strings"]
    if OUT.exists():
        shutil.rmtree(OUT)
    shutil.copytree(SOURCE, OUT)
    extract = (ROOT / "core" / "crates" / "capture" / "src" / "extract.js").read_text(encoding="utf-8")
    rules = (ROOT / "core" / "rules" / "sites.json").read_text(encoding="utf-8")
    (OUT / "capture.js").write_text(f"{extract.rstrip()}({rules.strip()})\n", encoding="utf-8")
    for locale, language in LOCALES.items():
        messages = {}
        for key, english in MESSAGES.items():
            text = english if language is None or english == "Paperloom" else strings[english][language]
            messages[key] = {"message": numbered(text)}
        folder = OUT / "_locales" / locale
        folder.mkdir(parents=True)
        (folder / "messages.json").write_text(json.dumps(messages, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    ZIP.unlink(missing_ok=True)
    with zipfile.ZipFile(ZIP, "w", zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(OUT.rglob("*")):
            if path.is_file():
                archive.write(path, path.relative_to(OUT).as_posix())
    print(f"{OUT.relative_to(ROOT)} and {ZIP.relative_to(ROOT)} written")


if __name__ == "__main__":
    main()
