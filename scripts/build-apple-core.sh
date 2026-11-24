#!/bin/bash
# Builds the Rust core (core/crates/core) for Apple platforms:
#   apple/CoreKit/Frameworks/PaperloomCoreFFI.xcframework   static libraries + C header
#   apple/CoreKit/Sources/CoreKit/Generated/                 UniFFI Swift bindings
#
#   scripts/build-apple-core.sh [--slices host|ios|macos|all[,...]] [--debug]
#
# --slices host   this Mac's architecture, for macOS and the iOS Simulator
#                 (tests and CI builds; the default)
# --slices ios    iPhone and iPad devices (App Store)
# --slices macos  Apple silicon and Intel Macs (App Store, Developer ID)
# --slices all    all of the above plus both Simulator architectures
#                 (several with commas, e.g. --slices host,ios)
# --debug         unoptimised build
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SLICES=host
PROFILE=release
while [ $# -gt 0 ]; do
  case "$1" in
    --slices) SLICES=${2:?--slices needs a value}; shift ;;
    --debug) PROFILE=debug ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done

if [ "$(uname -m)" = arm64 ]; then HOST_MAC=aarch64-apple-darwin; HOST_SIM=aarch64-apple-ios-sim
else HOST_MAC=x86_64-apple-darwin; HOST_SIM=x86_64-apple-ios; fi

MAC=(); SIM=(); IOS=()
for slice in ${SLICES//,/ }; do
  case "$slice" in
    host) MAC+=("$HOST_MAC"); SIM+=("$HOST_SIM") ;;
    ios) IOS+=(aarch64-apple-ios) ;;
    macos) MAC+=(aarch64-apple-darwin x86_64-apple-darwin) ;;
    all)
      MAC+=(aarch64-apple-darwin x86_64-apple-darwin)
      SIM+=(aarch64-apple-ios-sim x86_64-apple-ios)
      IOS+=(aarch64-apple-ios) ;;
    *) echo "unknown slices $SLICES" >&2; exit 2 ;;
  esac
done
# Each architecture once, as lipo wants.
uniq_targets() { printf '%s\n' "$@" | awk '!seen[$0]++'; }
if [ ${#MAC[@]} -gt 0 ]; then MAC=($(uniq_targets "${MAC[@]}")); fi
if [ ${#SIM[@]} -gt 0 ]; then SIM=($(uniq_targets "${SIM[@]}")); fi
if [ ${#IOS[@]} -gt 0 ]; then IOS=($(uniq_targets "${IOS[@]}")); fi
# The binding generator reads this Mac's dylib, so its target is always built.
ALL=("$HOST_MAC")
for t in ${MAC[@]+"${MAC[@]}"} ${SIM[@]+"${SIM[@]}"} ${IOS[@]+"${IOS[@]}"}; do
  [ "$t" = "$HOST_MAC" ] || ALL+=("$t")
done

CORE="$ROOT/apple/CoreKit"
WORK="$ROOT/target/apple"
OUT="$CORE/Frameworks/PaperloomCoreFFI.xcframework"
LIB=libpaperloom_core.a
# Match the apps' deployment targets so the C parts (SQLite) agree.
export MACOSX_DEPLOYMENT_TARGET=26.0 IPHONEOS_DEPLOYMENT_TARGET=26.0

rustup target add "${ALL[@]}" >/dev/null
CARGO_PROFILE=(--release); [ "$PROFILE" = debug ] && CARGO_PROFILE=()
for target in "${ALL[@]}"; do
  echo "==> cargo rustc $target ($PROFILE)"
  # The host Mac also needs the dylib, which the binding generator reads.
  types=staticlib; [ "$target" = "$HOST_MAC" ] && types=staticlib,cdylib
  cargo rustc -p paperloom-core --lib --crate-type "$types" \
    ${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"} --target "$target"
done

echo "==> Swift bindings"
GEN="$WORK/bindings"
rm -rf "$GEN" && mkdir -p "$GEN"
cargo run -p uniffi-bindgen --release -- generate \
  --library "$ROOT/target/$HOST_MAC/$PROFILE/libpaperloom_core.dylib" \
  --language swift --out-dir "$GEN"
mkdir -p "$WORK/headers" "$CORE/Sources/CoreKit/Generated"
cp "$GEN/paperloom_coreFFI.h" "$WORK/headers/"
cat > "$WORK/headers/module.modulemap" <<'EOF'
module paperloom_coreFFI {
    header "paperloom_coreFFI.h"
    export *
}
EOF
cp "$GEN/paperloom_core.swift" "$CORE/Sources/CoreKit/Generated/"

# One static library per platform: lipo the architectures together.
fat() {
  local name=$1; shift
  mkdir -p "$WORK/$name"
  local inputs=()
  for target in "$@"; do inputs+=("$ROOT/target/$target/$PROFILE/$LIB"); done
  lipo -create "${inputs[@]}" -output "$WORK/$name/$LIB"
  echo "$WORK/$name/$LIB"
}

echo "==> XCFramework"
ARGS=()
if [ ${#MAC[@]} -gt 0 ]; then ARGS+=(-library "$(fat macos "${MAC[@]}")" -headers "$WORK/headers"); fi
if [ ${#SIM[@]} -gt 0 ]; then ARGS+=(-library "$(fat ios-simulator "${SIM[@]}")" -headers "$WORK/headers"); fi
if [ ${#IOS[@]} -gt 0 ]; then ARGS+=(-library "$(fat ios "${IOS[@]}")" -headers "$WORK/headers"); fi
rm -rf "$OUT"
xcodebuild -create-xcframework "${ARGS[@]}" -output "$OUT"
echo "Built $OUT ($(du -sh "$OUT" | cut -f1))"
