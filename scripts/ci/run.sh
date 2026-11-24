#!/bin/bash
# Runs one CI job the same way on every CI service (CircleCI, Google Cloud
# Build, Azure Pipelines, GitHub Actions on Blacksmith); each service's
# config only picks the machine, passes the secrets and keeps dist/.
#
#   scripts/ci/run.sh core          Rust core: translations, format, clippy, tests
#   scripts/ci/run.sh server        sync service: format, clippy, tests (PostgreSQL)
#   scripts/ci/run.sh ui            web UI tests and build, extension zip, Tauri clippy
#   scripts/ci/run.sh linux-app     AppImage, deb and rpm into dist/
#   scripts/ci/run.sh android-app   signed APK into dist/
#   scripts/ci/run.sh android-test  x86_64 APK, share and phone checks on an emulator
#   scripts/ci/run.sh android-test-apk  only that APK, into dist/ (ANDROID_TEST_APK for the test)
#   scripts/ci/run.sh desktop-test  the web UI at desktop window sizes in Chromium and WebKit, with screenshots
#   scripts/ci/run.sh desktop-e2e   the installed Linux app (DESKTOP_APP: a folder with its .deb) driven end to end
#   scripts/ci/run.sh wps-e2e       WPS for Linux: the probe add-in, or with WPS_APP (the .deb) the app's add-in end to end
#   scripts/ci/run.sh windows-test  the core's tests on Windows
#   scripts/ci/run.sh windows-app   NSIS installer and MSI into dist/
#   scripts/ci/run.sh windows-e2e   the installed Windows app (WINDOWS_APP: a folder with its installer) driven end to end
#   scripts/ci/run.sh plugins       pinned releases of common plugins in the plugin runtimes
#
# With RELEASE_TAG set (e.g. v0.1.0) the app jobs first check that the tag
# matches the version everywhere, then publish dist/ to that GitHub release
# (GITHUB_RELEASE_TOKEN, GITHUB_REPOSITORY; see github_release.py).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
CI="$ROOT/scripts/ci"
cd "$ROOT"
JOB=${1:?job name}
export CARGO_TERM_COLOR=always CARGO_INCREMENTAL=0
# Windows has `python` (its `python3` may be the Microsoft Store stub);
# looked up when needed, since linux-deps.sh may be what installs it.
py() {
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) python "$@" ;;
    *) python3 "$@" ;;
  esac
}

# The checks all run even when one fails, so one run reports everything there
# is to fix; setup steps stop the job at once.
failed=()
check() {
  local name=$1; shift
  echo "==> $name"
  if ! "$@"; then failed+=("$name"); echo "FAILED: $name" >&2; fi
}
finish() {
  if [ ${#failed[@]} -gt 0 ]; then
    echo "Failed: ${failed[*]}" >&2
    exit 1
  fi
}

rust() {
  "$CI/setup-rust.sh" "$@"
  # A toolchain setup-rust.sh just installed is not on PATH yet.
  command -v cargo >/dev/null 2>&1 || export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
}
use_node() {
  local dir
  dir=$("$CI/setup-node.sh")
  if [ -n "$dir" ]; then export PATH="$dir:$PATH"; fi
}
npm_install() { (cd app && bash ../ci/npm-install.sh); }
icons() { (cd app && npx tauri icon app-icon.png); }
# THIRD_PARTY_NOTICES for the app's About section, served with the web UI
# (after npm_install: it reads the npm packages' licence texts).
notices() { py scripts/licenses.py notices app/public/THIRD_PARTY_NOTICES.txt --npm; }
# The Word add-in's manifest for the Windows package (the `word-addin`
# resource), once the add-in has a hosting address: PAPERLOOM_WORD_ADDIN_ORIGIN
# or integrations/word/origin.txt. Without one the folder stays empty and the
# app offers no Word add-in.
word_manifest() {
  rm -rf app/dist-word && mkdir -p app/dist-word
  py scripts/word-manifest.py app/dist-word/manifest.xml \
    --version "$(node -p "require('./app/src-tauri/tauri.conf.json').version")"
}
# cargo-deny's release binary, pinned and checked against its checksum.
cargo_deny() {
  local version=0.20.2 sha=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f dir
  dir=$(mktemp -d)
  curl -fsSL --retry 3 -o "$dir/deny.tar.gz" \
    "https://github.com/EmbarkStudios/cargo-deny/releases/download/$version/cargo-deny-$version-x86_64-unknown-linux-musl.tar.gz"
  echo "$sha  $dir/deny.tar.gz" | sha256sum -c --quiet
  tar -xzf "$dir/deny.tar.gz" -C "$dir" --strip-components=1
  CARGO_DENY="$dir/cargo-deny"
}

release_check() {
  if [ -n "${RELEASE_TAG:-}" ]; then
    py scripts/check_versions.py "${RELEASE_TAG#v}"
  fi
}
publish() {
  if [ -n "${RELEASE_TAG:-}" ]; then
    py "$CI/github_release.py" "$RELEASE_TAG" dist/*
  fi
}

# The release APK for one ABI, signed into dist/:
#   android_apk <tauri target> <Rust target> <name in the file>
android_apk() {
  local target=$1 triple=$2
  "$CI/linux-deps.sh" core
  rust "$triple"
  use_node
  npm_install
  : "${ANDROID_HOME:=${ANDROID_SDK_ROOT:?no Android SDK (ANDROID_HOME)}}"
  export ANDROID_HOME
  export NDK_HOME="${NDK_HOME:-$ANDROID_HOME/ndk/$(ls "$ANDROID_HOME/ndk" | sort -V | tail -1)}"
  # Be patient with slow Maven repositories and mirrors.
  export GRADLE_OPTS="${GRADLE_OPTS:-} -Dorg.gradle.internal.http.connectionTimeout=120000 -Dorg.gradle.internal.http.socketTimeout=180000 -Dorg.gradle.internal.network.retry.max.attempts=6"
  icons
  notices
  (cd app && npx tauri android init --ci && ../scripts/ci/android-mirrors.sh src-tauri/gen/android)
  # One retry: Gradle downloads occasionally time out.
  (cd app && { npx tauri android build --apk --target "$target" --ci || npx tauri android build --apk --target "$target" --ci; })
  rm -rf dist
  APK_ABI=$3 \
    KEYSTORE_B64="${KEYSTORE_B64:-${ANDROID_KEYSTORE_B64:-}}" \
    KEYSTORE_PASSWORD="${KEYSTORE_PASSWORD:-${ANDROID_KEYSTORE_PASSWORD:-}}" \
    KEY_ALIAS="${KEY_ALIAS:-${ANDROID_KEY_ALIAS:-}}" \
    "$CI/sign-apk.sh"
}

# Azure Pipelines passes an undefined variable on as the text "$(NAME)".
for var in GITHUB_RELEASE_TOKEN ANDROID_KEYSTORE_B64 ANDROID_KEYSTORE_PASSWORD ANDROID_KEY_ALIAS; do
  case "${!var:-}" in '$('*) unset "$var" ;; esac
done

case "$JOB" in
  core)
    "$CI/linux-deps.sh" core
    rust
    # With CI_REAL_FILES, Tesseract reads a drawn picture in the OCR crate's
    # tests and LibreOffice opens the documents the citation tests write.
    if [ -n "${CI_REAL_FILES:-}" ]; then
      export REAL_FILES=${REAL_FILES:-/tmp/paperloom-real}
      "$CI/real-files.sh" prepare
      export OCR_SAMPLE_PNG=$REAL_FILES/ocr.png WORDCITE_SAMPLES=$REAL_FILES/office
    fi
    check "Translations are complete" py scripts/i18n.py --check
    check "Format" cargo fmt --all --check
    check "Clippy" cargo clippy --all-targets --keep-going -- -D warnings
    check "Test" cargo test --no-fail-fast
    cargo_deny
    check "Licences and sources of Rust dependencies" "$CARGO_DENY" check licenses bans sources
    check "Security advisories of Rust dependencies" "$CARGO_DENY" check advisories
    if [ -n "${CI_REAL_FILES:-}" ]; then
      check "LibreOffice opens the cited documents" "$CI/real-files.sh" check
    fi
    finish
    ;;

  server)
    "$CI/linux-deps.sh" core
    rust
    # A service container may already provide the database.
    if [ -z "${PAPERLOOM_TEST_DATABASE_URL:-}" ]; then
      "$CI/postgres.sh"
      # The database postgres.sh made.
      export PAPERLOOM_TEST_DATABASE_URL="postgres://ci:ci@127.0.0.1:5432/paperloom_test?sslmode=disable"
    fi
    # The sync service is its own Cargo workspace (server/Cargo.toml).
    check "Sync service format" cargo fmt --manifest-path server/Cargo.toml --all --check
    check "Sync service clippy" cargo clippy --manifest-path server/Cargo.toml --all-targets --keep-going -- -D warnings
    check "Sync service tests" cargo test --manifest-path server/Cargo.toml --no-fail-fast
    finish
    ;;

  ui)
    use_node
    npm_install
    check "Typecheck" bash -c 'cd app && npm run typecheck'
    check "Licences of the web UI's npm packages" py scripts/licenses.py check-npm
    check "Tests" bash -c 'cd app && npm test'
    (cd app && npm run build)
    # The writing plugins' task pane, built alone as the add-ins load it.
    check "Task pane build" bash -c 'cd app && npx vite build --config vite.addin.config.ts'
    check "WPS add-in build" bash -c 'cd app && npx vite build --config vite.wps.config.ts'
    check "Word manifest" py scripts/word-manifest.py "$(mktemp -d)/manifest.xml" --origin https://example.pages.dev --version 0.1.0
    py scripts/build-extension.py
    # The Tauri app's Rust, checked against the UI just built (dist/).
    "$CI/linux-deps.sh" tauri
    rust
    icons
    check "Tauri app clippy" cargo clippy -p paperloom-app -- -D warnings
    finish
    ;;

  linux-app)
    release_check
    export APPIMAGE_EXTRACT_AND_RUN=1
    "$CI/linux-deps.sh" tauri
    rust
    use_node
    npm_install
    icons
    notices
    # LINUX_BUNDLES (e.g. deb) builds only those packages.
    (cd app && npx tauri build --ci ${LINUX_BUNDLES:+--bundles "$LINUX_BUNDLES"})
    rm -rf dist && mkdir -p dist
    find target/release/bundle -maxdepth 2 \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' \) -exec cp {} dist/ \;
    ls -lh dist
    publish
    ;;

  desktop-e2e)
    # The installed Linux app driven end to end (desktop-e2e.sh); DESKTOP_APP
    # is a folder with its .deb, as linux-app leaves in dist/.
    rust
    use_node
    "$CI/desktop-e2e.sh"
    ;;

  wps-e2e)
    # WPS for Linux in a virtual display (wps-e2e.sh): with WPS_APP (a folder
    # with the app's .deb) the app installs its add-in and WPS cites through
    # it; without, the probe add-in reports what WPS offers.
    if [ -n "${WPS_APP:-}" ]; then
      rust
      use_node
      # The add-in as this commit has it, in place of the package's (the
      # package may come from an earlier run; its app side is the same).
      npm_install
      (cd app && npx vite build --config vite.wps.config.ts)
      export WPS_ADDIN_BUILD=$ROOT/app/dist-wps
    fi
    "$CI/wps-e2e.sh"
    ;;

  android-app)
    release_check
    android_apk aarch64 aarch64-linux-android arm64
    publish
    ;;

  android-test-apk)
    # The x86_64 APK for android-test, when another job runs the emulator
    # (Azure: each job has 60 minutes).
    android_apk x86_64 x86_64-linux-android x86_64
    ;;

  android-test)
    # An x86_64 build, which the emulator runs natively: the one in
    # ANDROID_TEST_APK (a folder) if an earlier job built it, else built here.
    if [ -n "${ANDROID_TEST_APK:-}" ]; then
      apk=$(find "$ANDROID_TEST_APK" -name '*.apk' | head -1)
      [ -n "$apk" ] || { echo "no APK in $ANDROID_TEST_APK" >&2; exit 1; }
    else
      android_apk x86_64 x86_64-linux-android x86_64
      apk=$(find dist -maxdepth 1 -name '*.apk' | head -1)
    fi
    # Source the parts so failures and warnings share one summary and log.
    source "$CI/android-emulator.sh"
    android_test_setup
    if ! android_start_emulator; then fail "starting the emulator"; exit 1; fi
    source "$CI/android-share-test.sh"
    source "$CI/android-phone-test.sh"
    # Conditional calls disable errexit inside the tests: a failed step must
    # leave its artifacts and let the other steps run.
    android_share_test "$apk" || fail "share test did not complete"
    android_phone_test || fail "phone test did not complete"
    android_finish
    ;;

  desktop-test)
    use_node
    npm_install
    (cd app && npm run build)
    # Playwright only here, outside the app's dependencies, with the
    # browsers and the system libraries they need.
    PLAYWRIGHT_DIR=$(mktemp -d)
    npm install --no-save --no-package-lock --no-audit --no-fund --prefix "$PLAYWRIGHT_DIR" \
      "playwright@${PLAYWRIGHT_VERSION:-1.63.0}" "axe-core@${AXE_VERSION:-4.10.3}" >/dev/null
    (cd "$PLAYWRIGHT_DIR" && npx playwright install --with-deps chromium webkit)
    PLAYWRIGHT_MODULE="$PLAYWRIGHT_DIR/node_modules/playwright/index.mjs" \
      AXE_SCRIPT="$PLAYWRIGHT_DIR/node_modules/axe-core/axe.min.js" \
      node scripts/ci/desktop-screens.mjs app/dist test-results/desktop
    ;;

  windows-test)
    rust
    use_node
    npm_install
    icons
    cargo test
    ;;

  windows-app)
    release_check
    rust
    use_node
    npm_install
    icons
    notices
    word_manifest
    # WINDOWS_BUNDLES (e.g. nsis) builds only those installers.
    (cd app && npx tauri build --ci ${WINDOWS_BUNDLES:+--bundles "$WINDOWS_BUNDLES"})
    rm -rf dist && mkdir -p dist
    # The NSIS installer (all six languages) is the main download; the
    # English MSI is for deployment tools.
    for installer in target/release/bundle/msi/*.msi target/release/bundle/nsis/*.exe; do
      if [ -e "$installer" ]; then cp "$installer" dist/; fi
    done
    ls -l dist
    publish
    ;;

  windows-e2e)
    # The installed Windows app driven end to end (windows-e2e.ps1);
    # WINDOWS_APP is a folder with its NSIS installer.
    use_node
    pwsh -NoProfile -File "$CI/windows-e2e.ps1" -Dist "${WINDOWS_APP:?a folder with the installer}" -Out test-results/desktop-e2e
    ;;

  plugins)
    "$CI/linux-deps.sh" core
    use_node
    npm_install
    (cd app && bash ../scripts/ci/plugin-compat.sh)
    ;;

  *)
    echo "unknown job $JOB" >&2
    exit 2
    ;;
esac
