#!/usr/bin/env python3
"""Generates the apps' translations from i18n/strings.json, the one place
where they are written:

    apple/Paperloom/Resources/Localizable.xcstrings   (String Catalog)
    apple/Paperloom/Resources/InfoPlist.xcstrings     (app name)
    app/src/i18n/locales/<language>.json               (Tauri app)
    core/crates/core/src/messages.json                 (Core's error messages)

    python3 scripts/i18n.py           write the files
    python3 scripts/i18n.py --check   fail if a translation is missing, a
                                      placeholder differs, or a generated
                                      file is out of date (CI)

Keys are the English text. Placeholders are written {name}; the String
Catalog gets %@ in their place.

The "core" section holds the messages Core and the apps' Rust code write
in English (errors, mostly); Core matches a message against these keys and
translates it (core/crates/core/src/messages.rs). --check also reads the
Rust sources and fails when a message there has no translation, unless it
is listed in "coreUntranslated" (technical text nobody reads).
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "i18n/strings.json"
CATALOG = ROOT / "apple/Paperloom/Resources/Localizable.xcstrings"
INFOPLIST = ROOT / "apple/Paperloom/Resources/InfoPlist.xcstrings"
LOCALES = ROOT / "app/src/i18n/locales"
CORE_MESSAGES = ROOT / "core/crates/core/src/messages.json"
# Rust code whose messages reach people.
RUST_SOURCES = [ROOT / "core/crates", ROOT / "app/src-tauri/src", ROOT / "app/src-tauri/plugins"]

LANGUAGES = ["zh-Hans", "zh-Hant", "fr", "ja", "ru"]  # besides English, the source
PLACEHOLDER = re.compile(r"\{(\w+)\}")


def catalog(strings):
    return {
        "sourceLanguage": "en",
        "strings": {
            PLACEHOLDER.sub("%@", key): {
                "extractionState": "manual",
                "localizations": {
                    lang: {"stringUnit": {"state": "translated", "value": PLACEHOLDER.sub("%@", text)}}
                    for lang, text in sorted(translations.items())
                },
            }
            for key, translations in sorted(strings.items())
        },
        "version": "1.0",
    }


def dump(data):
    return json.dumps(data, ensure_ascii=False, indent=2) + "\n"


def outputs(source):
    files = {
        CATALOG: dump(catalog(source["strings"])),
        INFOPLIST: dump(catalog(source["infoPlist"])),
    }
    for lang in LANGUAGES:
        messages = {key: t[lang] for key, t in sorted(source["strings"].items())}
        files[LOCALES / f"{lang}.json"] = dump(messages)
    files[CORE_MESSAGES] = dump(
        {lang: {key: t[lang] for key, t in sorted(source["core"].items())} for lang in LANGUAGES}
    )
    return files


# A message in Rust: an error type's text, a `message:` field, or what an
# error helper or `Err(…)` is given; plain or through `format!`.
RUST_MESSAGE = re.compile(
    r'(?:#\[error\(|\bmessage:\s*|\b(?:invalid|storage|failed|unreadable|io_error|not_found|NotFound|NetworkError|Setup|Err)\(\s*)'
    r'(?:format!\(\s*)?"((?:[^"\\]|\\.)*)"'
)
RUST_ESCAPE = re.compile(r"\\(u\{[0-9a-fA-F]+\}|\n\s*|.)")
FORMAT_FIELD = re.compile(r"\{\{|\}\}|\{([A-Za-z_][A-Za-z0-9_]*|\d+)?(?::[^}]*)?\}")


def rust_text(literal):
    """A Rust string literal's text (escapes undone)."""

    def undo(match):
        code = match.group(1)
        if code.startswith("u{"):
            return chr(int(code[2:-1], 16))
        if code.startswith("\n"):  # line continuation
            return ""
        return {"n": "\n", "t": "\t", "0": "\0"}.get(code, code)

    return RUST_ESCAPE.sub(undo, literal)


def template(format_string):
    """A `format!` string as a catalog key: `{}` and `{:?}` become {0},
    {1}…, `{name:…}` becomes {name}, `{{` a brace."""
    position = 0

    def field(match):
        nonlocal position
        whole = match.group(0)
        if whole == "{{":
            return "{"
        if whole == "}}":
            return "}"
        name = match.group(1)
        if name is None:
            name = str(position)
            position += 1
        return "{" + name + "}"

    return FORMAT_FIELD.sub(field, format_string)


def letters(text):
    return sum(ch.isalpha() for ch in text)


def rust_messages():
    """Message keys found in the Rust sources, with where each is."""
    found = {}
    for root in RUST_SOURCES:
        for path in sorted(root.rglob("*.rs")):
            if "target" in path.parts or "tests" in path.parts or path.stem == "tests":
                continue
            code = path.read_text(encoding="utf-8").split("#[cfg(test)]")[0]
            for match in RUST_MESSAGE.finditer(code):
                key = template(rust_text(match.group(1)))
                if letters(PLACEHOLDER.sub("", key)) >= 3:
                    line = code.count("\n", 0, match.start()) + 1
                    found.setdefault(key, f"{path.relative_to(ROOT)}:{line}")
    return found


def core_problems(source):
    found = []
    for key in source["core"]:
        parts = PLACEHOLDER.split(key)
        literal = "".join(parts[0::2])
        # Core finds the values between the literal parts: a key needs
        # enough of them, and something other than spaces between two values.
        if PLACEHOLDER.search(key) and letters(literal) < 3:
            found.append(f"core: {key!r} has too little text around its placeholders")
        if any(not part.strip() for part in parts[2:-1:2]):
            found.append(f"core: {key!r} has placeholders with only spaces between them")
    skip = set(source.get("coreUntranslated", []))
    for key, where in rust_messages().items():
        if key not in source["core"] and key not in skip:
            found.append(f"core: {where}: {key!r} has no translation")
    return found


def problems(source):
    found = []
    # Info.plist keys are key names, not English text: English is written
    # out too, or the English name of the app would be the key itself.
    for section, languages in (("strings", LANGUAGES), ("core", LANGUAGES), ("infoPlist", ["en", *LANGUAGES])):
        for key, translations in source[section].items():
            wanted = set(PLACEHOLDER.findall(key))
            for lang in languages:
                text = translations.get(lang)
                if not text:
                    found.append(f"{section}: {key!r} has no {lang} translation")
                elif set(PLACEHOLDER.findall(text)) != wanted:
                    found.append(f"{section}: {key!r} in {lang} has placeholders {PLACEHOLDER.findall(text)}")
            extra = set(translations) - set(languages)
            if extra:
                found.append(f"{section}: {key!r} has unknown languages {sorted(extra)}")
    return found + core_problems(source)


def main():
    source = json.loads(SOURCE.read_text(encoding="utf-8"))
    source.setdefault("core", {})
    issues = problems(source)
    check = "--check" in sys.argv[1:]
    for path, text in outputs(source).items():
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                issues.append(f"{path.relative_to(ROOT)} is out of date: run scripts/i18n.py")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
    for issue in issues:
        print(f"error: {issue}", file=sys.stderr)
    if issues:
        sys.exit(1)
    count = len(source["strings"]) + len(source["core"])
    print(f"{count} strings in {len(LANGUAGES) + 1} languages{' are up to date' if check else ' written'}")


if __name__ == "__main__":
    main()
