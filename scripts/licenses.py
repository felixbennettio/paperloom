#!/usr/bin/env python3
"""Third-party licences: the check of the web UI's npm dependencies, and the
notices the apps ship.

  python3 scripts/licenses.py check-npm
      Every npm package the web UI ships (app/package-lock.json, dev
      dependencies left out) has a licence deny.toml allows (the Rust
      dependencies are checked by cargo-deny against the same list).
  python3 scripts/licenses.py notices <out.txt> [--npm]
      THIRD_PARTY_NOTICES: each Rust crate the apps use (cargo metadata) and,
      with --npm, each npm package the web UI ships (from node_modules, so
      after npm ci), with its licence and the licence texts it carries. A text
      shared by many packages is written once, with all of them listed.
"""
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|COPYRIGHT|UNLICENSE)([-._].*)?$", re.IGNORECASE)


def allowed() -> set[str]:
    # Here, not at the top: notices also runs on Xcode Cloud's older Python.
    import tomllib

    return set(tomllib.loads((ROOT / "deny.toml").read_text(encoding="utf-8"))["licenses"]["allow"])


def tokens(expression: str) -> list[str]:
    # Old npm and cargo spellings: "MIT/Apache-2.0" means either.
    expression = expression.replace("/", " OR ")
    return re.findall(r"\(|\)|[^\s()]+", expression)


def satisfied(expression: str, allow: set[str]) -> bool:
    """Whether an SPDX expression can be met with allowed licences only."""
    items = tokens(expression)
    position = [0]

    def peek():
        return items[position[0]] if position[0] < len(items) else None

    def take():
        position[0] += 1
        return items[position[0] - 1]

    # Each operand is read before the result is combined, so every token is used.
    def term():
        token = take()
        if token == "(":
            value = either()
            take()  # ")"
            return value
        name = token.removesuffix("+")
        if peek() == "WITH":
            take()
            name = f"{name} WITH {take()}"
        return name in allow

    def both():
        value = term()
        while peek() == "AND":
            take()
            value = term() and value
        return value

    def either():
        value = both()
        while peek() == "OR":
            take()
            value = both() or value
        return value

    return either()


def npm_packages() -> list[tuple[str, str, str]]:
    """(name, version, licence) of each npm package the web UI ships."""
    lock = json.loads((ROOT / "app/package-lock.json").read_text(encoding="utf-8"))
    shipped = []
    for path, package in lock["packages"].items():
        if not path or package.get("dev") or package.get("link"):
            continue
        name = path.split("node_modules/")[-1]
        licence = package.get("license") or ""
        if isinstance(licence, dict):
            licence = licence.get("type", "")
        shipped.append((name, package.get("version", ""), licence))
    return sorted(shipped)


def check_npm() -> int:
    allow = allowed()
    bad = [(n, v, l) for n, v, l in npm_packages() if not l or not satisfied(l, allow)]
    for name, version, licence in bad:
        print(f"{name} {version}: {licence or 'no licence given'}", file=sys.stderr)
    count = len(npm_packages())
    print(f"npm packages shipped: {count}; not allowed: {len(bad)}")
    return 1 if bad else 0


def licence_texts(folder: Path) -> list[str]:
    if not folder.is_dir():
        return []
    texts = []
    for path in sorted(folder.iterdir()):
        if path.is_file() and LICENSE_FILE.match(path.name):
            texts.append(path.read_text(encoding="utf-8", errors="replace").strip())
    return texts


def notices(out: Path, npm: bool) -> int:
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--locked"], cwd=ROOT))
    own = set(metadata["workspace_members"])
    entries = []  # (heading, licence, texts)
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["id"] in own:
            continue
        heading = f"{package['name']} {package['version']} (Rust)"
        if package.get("repository"):
            heading += f" — {package['repository']}"
        entries.append((heading, package.get("license") or "", licence_texts(Path(package["manifest_path"]).parent)))
    if npm:
        for name, version, licence in npm_packages():
            entries.append((f"{name} {version} (npm)", licence, licence_texts(ROOT / "app/node_modules" / name)))

    # Each distinct text once, after the packages that carry it.
    by_text: dict[str, list[str]] = {}
    texts: dict[str, str] = {}
    without = []
    for heading, licence, found in entries:
        if not found:
            without.append(f"{heading}: {licence}")
            continue
        for text in found:
            key = hashlib.sha256(text.encode()).hexdigest()
            texts[key] = text
            by_text.setdefault(key, []).append(f"{heading}: {licence}")
    lines = [
        "THIRD-PARTY NOTICES",
        "",
        "Paperloom includes the following open-source software. Each list of",
        "packages is followed by the licence text they carry.",
        "",
    ]
    for key, packages in by_text.items():
        lines += ["=" * 78, *packages, "-" * 78, texts[key], ""]
    if without:
        lines += ["=" * 78, "Packages whose licence is named but whose text they do not carry:", *without, ""]
    out.parent.mkdir(parents=True, exist_ok=True)
    # UTF-8 everywhere: Windows would write its own code page.
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {out}: {len(entries)} packages, {len(texts)} distinct licence texts")
    return 0


def main(args: list[str]) -> int:
    if args[:1] == ["check-npm"]:
        return check_npm()
    if args[:1] == ["notices"] and len(args) >= 2:
        return notices(Path(args[1]), "--npm" in args[2:])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
