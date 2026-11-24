#!/usr/bin/env python3
"""Materialize the Word manifest: word-manifest.py output.xml [--origin https://host].

Without --origin the add-in's address is PAPERLOOM_WORD_ADDIN_ORIGIN or the
one in integrations/word/origin.txt; when neither has one (the add-in is
not hosted yet) no manifest is written, and the app offers no Word add-in.
"""

import argparse
import json
import os
import re
from pathlib import Path
from urllib.parse import urlsplit
import xml.etree.ElementTree as ET
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parent.parent
APP_NS = "http://schemas.microsoft.com/office/appforoffice/1.1"
BT_NS = "http://schemas.microsoft.com/office/officeappbasictypes/1.0"
VO_NS = "http://schemas.microsoft.com/office/taskpaneappversionoverrides"
XSI_NS = "http://www.w3.org/2001/XMLSchema-instance"
LOCALES = {"zh-Hans": "zh-CN", "zh-Hant": "zh-TW", "fr": "fr-FR", "ja": "ja-JP", "ru": "ru-RU"}


def origin(value):
    try:
        parsed = urlsplit(value)
        port = parsed.port
    except ValueError as error:
        raise argparse.ArgumentTypeError("Invalid HTTPS origin") from error
    if (parsed.scheme != "https" or not parsed.hostname or parsed.path or parsed.query
            or parsed.fragment or parsed.username is not None or parsed.password is not None
            or value.endswith("/") or any(char.isspace() for char in value)
            or any(char in value for char in '\\<>"{}') or port == 0
            or value != f"https://{parsed.netloc}"):
        raise argparse.ArgumentTypeError("Use an HTTPS origin without credentials, a path, query, fragment or trailing slash")
    return value


def hosted_origin():
    """The address the add-in is hosted at, or None before it is."""
    value = os.environ.get("PAPERLOOM_WORD_ADDIN_ORIGIN", "").strip()
    if not value:
        lines = (ROOT / "integrations/word/origin.txt").read_text(encoding="utf-8").splitlines()
        value = next((line.strip() for line in lines if line.strip() and not line.startswith("#")), "")
    return origin(value) if value else None


def version(value):
    if not re.fullmatch(r"\d+\.\d+\.\d+", value):
        raise argparse.ArgumentTypeError("Use the app's version, such as 0.1.0")
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--origin", type=origin)
    # Office keeps an add-in's files by its manifest's version.
    parser.add_argument("--version", type=version)
    args = parser.parse_args()
    if ".git" in args.output.resolve().parts:
        parser.error("The output must not be inside .git")
    try:
        address = args.origin or hosted_origin()
    except argparse.ArgumentTypeError as error:
        parser.error(f"integrations/word/origin.txt: {error}")
    if not address:
        print("The Word add-in has no hosting address yet: no manifest written")
        return
    template = (ROOT / "integrations/word/manifest.xml").read_text(encoding="utf-8")
    if args.version:
        template = template.replace("<Version>0.1.0.0</Version>", f"<Version>{args.version}.0</Version>")
    manifest = ET.fromstring(template.replace("{{ORIGIN}}", escape(address, {'"': "&quot;"})))
    strings = json.loads((ROOT / "i18n/strings.json").read_text(encoding="utf-8"))["strings"]
    # Office reads manifest strings before the task pane's React translations.
    for element in manifest.iter():
        if element.tag not in (f"{{{APP_NS}}}Description", f"{{{BT_NS}}}String"):
            continue
        translations = strings.get(element.get("DefaultValue"))
        if translations:
            namespace = element.tag.split("}")[0][1:]
            for language, locale in LOCALES.items():
                ET.SubElement(element, f"{{{namespace}}}Override", Locale=locale, Value=translations[language])
    ET.register_namespace("", APP_NS)
    ET.register_namespace("bt", BT_NS)
    ET.register_namespace("xsi", XSI_NS)
    ET.register_namespace("ov", VO_NS)
    # Keep VersionOverrides' default namespace: xsi:type values use it too.
    version_overrides = manifest.find(f"{{{VO_NS}}}VersionOverrides")
    for element in version_overrides.iter():
        if element.tag.startswith(f"{{{VO_NS}}}"):
            element.tag = element.tag.split("}", 1)[1]
    version_overrides.set("xmlns", VO_NS)
    ET.indent(manifest, space="  ")
    rendered = ET.tostring(manifest, encoding="utf-8", xml_declaration=True)
    ET.fromstring(rendered)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(rendered + b"\n")
    ET.parse(args.output)
    print(f"Word manifest written to {args.output}")


if __name__ == "__main__":
    main()
