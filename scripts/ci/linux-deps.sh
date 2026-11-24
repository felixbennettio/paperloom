#!/bin/bash
# Installs the system packages a Linux CI job needs.
#
#   scripts/ci/linux-deps.sh core    C toolchain for the Rust core (bundled SQLite)
#   scripts/ci/linux-deps.sh tauri   plus WebKitGTK and friends for the Tauri app
set -euo pipefail

packages=(build-essential pkg-config curl file python3 xz-utils)
if [ "${1:-core}" = tauri ]; then
  packages+=(libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev
    libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf wget xdg-utils)
fi

# Only what is missing: the CI image comes with all of them.
missing=()
for p in "${packages[@]}"; do
  dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q 'install ok installed' || missing+=("$p")
done
if [ ${#missing[@]} -eq 0 ]; then
  echo "already installed: ${packages[*]}"
  exit 0
fi
packages=("${missing[@]}")

SUDO=""
if [ "$(id -u)" != 0 ]; then SUDO=sudo; fi
export DEBIAN_FRONTEND=noninteractive
# A self-hosted runner may name an Ubuntu mirror (CI_APT_MIRROR); GitHub's
# images use their own.
if [ -n "${CI_APT_MIRROR:-}" ] && [ -f /etc/apt/sources.list ]; then
  $SUDO sed -i -E "s#https?://(archive|security)\.ubuntu\.com/ubuntu/?#${CI_APT_MIRROR%/}/#g" /etc/apt/sources.list
  echo "apt via $CI_APT_MIRROR"
fi
$SUDO apt-get update -qq
$SUDO apt-get install -y -qq --no-install-recommends "${packages[@]}" >/dev/null
echo "installed: ${packages[*]}"
