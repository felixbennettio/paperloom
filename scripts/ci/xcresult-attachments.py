#!/usr/bin/env python3
"""Copies the attachments (screenshots, text) out of an Xcode result bundle
without Xcode, e.g. the RESULT_BUNDLE artifact of the Xcode Cloud workflow
"UI Screens" (docs/ci.md). Needs Python 3.14 for zstd.

  scripts/ci/xcresult-attachments.py <bundle.xcresult or its .zip> <output folder>

Files are named after the attachment (the UI tests name them
device-NN-screen); images get .png, other attachments keep their name.
"""
import re
import sqlite3
import sys
import tempfile
import zipfile
from compression import zstd
from pathlib import Path

PNG = b"\x89PNG\r\n\x1a\n"
JPEG = b"\xff\xd8\xff"
ZSTD = b"\x28\xb5\x2f\xfd"


def bundle_root(path: Path, work: Path) -> Path:
    if path.is_dir():
        return path
    with zipfile.ZipFile(path) as archive:
        archive.extractall(work)
    found = next((p for p in work.rglob("*.xcresult") if p.is_dir()), None)
    if found is None:
        found = next((p.parent for p in work.rglob("Info.plist") if (p.parent / "Data").is_dir()), None)
    if found is None:
        sys.exit("no result bundle in the zip")
    return found


def payload(root: Path, ref: str) -> bytes | None:
    file = root / "Data" / f"data.{ref}"
    if not file.exists():
        return None
    data = file.read_bytes()
    if data.startswith(ZSTD):
        data = zstd.decompress(data)
    return data


def field(record: bytes, *keys: bytes) -> str | None:
    """A string field of a serialized object, `K<n>:<key>[...V<len>:<value>`,
    following nested keys (payloadRef, then its id)."""
    start = 0
    for key in keys:
        match = re.compile(rb"K\d+:" + re.escape(key) + rb"\[").search(record, start)
        if not match:
            return None
        start = match.end()
    match = re.compile(rb"V(\d+):").search(record, start)
    if not match:
        return None
    start = match.end()
    return record[start : start + int(match.group(1))].decode(errors="replace")


def attachments_in_objects(root: Path):
    """(name, payload id) for each attachment, found in the serialized test
    summaries (bundles without a database, as Xcode Cloud keeps them)."""
    # A type's first object in a file names it in full (`V20:`), later
    # ones refer back to it (`S20:`).
    marker = re.compile(rb"[SV]20:ActionTestAttachment")
    seen = set()
    for file in (root / "Data").iterdir():
        data = file.read_bytes()
        if data.startswith(ZSTD):
            data = zstd.decompress(data)
        if b"20:ActionTestAttachment" not in data:
            continue
        for record in marker.split(data)[1:]:
            name = field(record, b"name")
            ref = field(record, b"payloadRef", b"id")
            if ref and ref not in seen:
                seen.add(ref)
                yield name or "attachment", ref


def attachments(db: sqlite3.Connection):
    """(name, payload id) for each attachment, from the bundle's database
    (Xcode 16 and later, when the bundle has one)."""
    tables = [row[0] for row in db.execute("select name from sqlite_master where type = 'table'")]
    table = next((t for t in tables if t.lower() == "attachments"), None)
    if table is None:
        sys.exit(f"no Attachments table; tables: {', '.join(tables)}")
    columns = [row[1] for row in db.execute(f"pragma table_info({table})")]
    ref = next((c for c in columns if "payloadref" in c.lower()), None)
    name = next((c for c in columns if c.lower() == "name"), None)
    if ref is None or name is None:
        sys.exit(f"unexpected Attachments columns: {', '.join(columns)}")
    for row in db.execute(f"select {name}, {ref} from {table}"):
        yield row[0] or "attachment", row[1]


def safe(name: str) -> str:
    return "".join(c if c.isalnum() or c in "-_." else "_" for c in name)[:120]


def main():
    source, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as work:
        root = bundle_root(source, Path(work))
        database = root / "database.sqlite3"
        if database.exists():
            found = attachments(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True))
        else:
            found = attachments_in_objects(root)
        count = 0
        for name, ref in found:
            # XCTest's own records (each UI step, a screen recording).
            if name.startswith("kXCTAttachment"):
                continue
            data = payload(root, ref) if ref else None
            if data is None:
                print(f"missing payload: {name}")
                continue
            stem = safe(name)
            if data.startswith(PNG):
                target = out / (stem if stem.endswith(".png") else stem + ".png")
            elif data.startswith(JPEG):
                target = out / (stem if stem.endswith(".jpeg") else stem + ".jpeg")
            else:
                target = out / stem
            # Two devices may give the same name; keep both.
            base, n = target, 2
            while target.exists():
                target = base.with_stem(f"{base.stem}-{n}")
                n += 1
            target.write_bytes(data)
            count += 1
        print(f"{count} attachments in {out}")


if __name__ == "__main__":
    main()
