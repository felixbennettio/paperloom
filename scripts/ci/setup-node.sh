#!/bin/bash
# Makes sure Node.js 24 or newer is available on a Linux CI machine, from
# nodejs.org (checked against its published SHA-256) when the image has none.
# Prints the directory to put first on PATH, or nothing when the image's own
# Node will do. Everything else goes to stderr.
#
#   PATH="$(scripts/ci/setup-node.sh):$PATH"
set -euo pipefail

MAJOR=24
if command -v node >/dev/null 2>&1; then
  have=$(node -p 'process.versions.node.split(".")[0]')
  if [ "$have" -ge "$MAJOR" ]; then
    echo "Node $(node --version)" >&2
    exit 0
  fi
fi
if [ "$(uname -s)" != Linux ]; then
  echo "Node $MAJOR+ is missing; install it in the CI config for this platform" >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64) ARCH=x64 ;;
  aarch64 | arm64) ARCH=arm64 ;;
  *) echo "no Node build for $(uname -m)" >&2; exit 1 ;;
esac
BASE="https://nodejs.org/dist/latest-v$MAJOR.x"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
curl -fsSL --retry 3 -o "$WORK/SHASUMS256.txt" "$BASE/SHASUMS256.txt"
FILE=$(sed -n "s/^[0-9a-f]\{64\}  \(node-v[0-9.]*-linux-$ARCH\.tar\.xz\)\$/\1/p" "$WORK/SHASUMS256.txt")
[ -n "$FILE" ] || { echo "no linux-$ARCH tarball in $BASE" >&2; exit 1; }
curl -fsSL --retry 3 -o "$WORK/$FILE" "$BASE/$FILE"
(cd "$WORK" && grep "  $FILE\$" SHASUMS256.txt | sha256sum -c - >&2)
DEST="$HOME/.local/${FILE%.tar.xz}"
mkdir -p "$DEST"
tar -xJf "$WORK/$FILE" -C "$DEST" --strip-components 1
echo "Node $("$DEST/bin/node" --version) from nodejs.org" >&2
echo "$DEST/bin"
