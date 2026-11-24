#!/usr/bin/env python3
"""Keeps the app version the same everywhere it is written.

    python3 scripts/check_versions.py            print the versions
    python3 scripts/check_versions.py 0.1.0      fail unless all are 0.1.0 (release.yml)
    python3 scripts/check_versions.py --set 0.1.0  write 0.1.0 everywhere
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# file -> (pattern whose group 1 is the version)
PLACES = {
    "Cargo.toml": r'(?ms)^\[workspace\.package\].*?^version = "([^"]+)"',
    "app/src-tauri/tauri.conf.json": r'"version": "([^"]+)"',
    "app/package.json": r'"version": "([^"]+)"',
    "apple/project.yml": r'MARKETING_VERSION: "([^"]+)"',
}


def read():
    versions = {}
    for name, pattern in PLACES.items():
        match = re.search(pattern, (ROOT / name).read_text(encoding="utf-8"))
        versions[name] = match.group(1) if match else None
    return versions


def write(version):
    if not re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?", version):
        sys.exit(f"not a version: {version}")
    for name, pattern in PLACES.items():
        path = ROOT / name
        text = path.read_text(encoding="utf-8")
        match = re.search(pattern, text)
        if not match:
            sys.exit(f"{name}: version not found")
        path.write_text(text[: match.start(1)] + version + text[match.end(1) :], encoding="utf-8")
    json.loads((ROOT / "app/package.json").read_text(encoding="utf-8"))  # still valid


def main():
    args = sys.argv[1:]
    if args[:1] == ["--set"] and len(args) == 2:
        write(args[1])
    versions = read()
    for name, version in versions.items():
        print(f"{version or '?':>12}  {name}")
    expected = args[-1] if args else None
    wrong = [n for n, v in versions.items() if v != (expected or versions["Cargo.toml"])]
    if wrong:
        sys.exit(f"error: version mismatch in {', '.join(wrong)} (expected {expected or versions['Cargo.toml']})")


if __name__ == "__main__":
    main()
