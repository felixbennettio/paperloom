#!/bin/bash
# Xcode Cloud, after the clone (every action runs it on its own machine):
# builds the Rust core for the action's platform and generates the Xcode
# project, which is not committed. Which workflows to set up: docs/ci.md.
set -euo pipefail

cd "${CI_PRIMARY_REPOSITORY_PATH:?not running in Xcode Cloud}"

if [ -n "${CI_TAG:-}" ]; then
  python3 scripts/check_versions.py "${CI_TAG#v}"
fi

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf --retry 3 https://sh.rustup.rs |
    sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable
  export PATH="$HOME/.cargo/bin:$PATH"
fi
rustc --version
HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 brew install --quiet xcodegen

# Only the slices this action links: devices for iOS builds and archives,
# the Simulator as well for iOS tests, and both Mac architectures for macOS
# ("Any Mac" builds both; the CoreKit tests below use this Mac's).
case "$CI_PRODUCT_PLATFORM-$CI_XCODEBUILD_ACTION" in
  iOS-archive | iOS-build | iOS-analyze) SLICES=ios ;;
  iOS-*) SLICES=host,ios ;;
  *) SLICES=macos ;;
esac
scripts/build-apple-core.sh --slices "$SLICES"

if [ "$CI_PRODUCT_PLATFORM" = macOS ] && [ "$CI_XCODEBUILD_ACTION" != archive ]; then
  swift test --package-path apple/CoreKit
fi

# The Developer ID workflow sets PAPERLOOM_DIRECT=1: no App Sandbox. Swap
# the file rather than overriding CODE_SIGN_ENTITLEMENTS, which would apply
# to the Swift package targets as well.
if [ "${PAPERLOOM_DIRECT:-}" = 1 ]; then
  cp apple/Paperloom/Support/Paperloom-macOS-Direct.entitlements apple/Paperloom/Support/Paperloom-macOS.entitlements
fi

# The Rust crates the app is built with and their licences, for Settings >
# About (scripts/licenses.py; not committed).
python3 scripts/licenses.py notices apple/Paperloom/Resources/THIRD_PARTY_NOTICES.txt

# The Word add-in's manifest for the Mac app's Settings, once the add-in has
# a hosting address (scripts/word-manifest.py; not committed).
if [ "$CI_PRODUCT_PLATFORM" = macOS ]; then
  python3 scripts/word-manifest.py apple/Paperloom/Resources/WordAddinManifest.xml \
    --version "$(python3 -c 'import json; print(json.load(open("app/src-tauri/tauri.conf.json"))["version"])')"
fi

cd apple
xcodegen generate

# The UI test plans name their targets by the generated project's ids.
python3 - <<'PLAN'
import pathlib, re
project = pathlib.Path("Paperloom.xcodeproj/project.pbxproj").read_text()
for target, mark, plans in [
    ("Paperloom-iOSScreens", "PAPERLOOM_SCREENS_TARGET", ["PaperloomScreens", "PaperloomStore"]),
    ("Paperloom-macOSScreens", "PAPERLOOM_MAC_SCREENS_TARGET", ["PaperloomStoreMac"]),
]:
    found = re.search(r"([0-9A-F]{24}) /\* " + target + r" \*/ = \{\s*isa = PBXNativeTarget;", project)
    if not found:
        raise SystemExit(f"{target} is not in the generated project")
    for name in plans:
        plan = pathlib.Path(f"{name}.xctestplan")
        plan.write_text(plan.read_text().replace(mark, found.group(1)))
PLAN
