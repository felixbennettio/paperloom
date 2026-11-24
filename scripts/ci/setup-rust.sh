#!/bin/bash
# Installs the stable Rust toolchain on a CI runner (Linux or macOS) unless
# it is already there (GitHub's images ship one), and adds the given extra
# targets. rustup honours RUSTUP_DIST_SERVER / RUSTUP_UPDATE_ROOT when a
# self-hosted runner names a mirror.
#
#   scripts/ci/setup-rust.sh [target...]
set -euo pipefail

if ! command -v rustup >/dev/null 2>&1; then
  export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
  curl --proto '=https' --tlsv1.2 -sSf --retry 3 https://sh.rustup.rs |
    sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable --component rustfmt,clippy
  export PATH="$CARGO_HOME/bin:$PATH"
  if [ -n "${GITHUB_PATH:-}" ]; then echo "$CARGO_HOME/bin" >> "$GITHUB_PATH"; fi
  if [ -n "${GITHUB_ENV:-}" ]; then
    echo "CARGO_HOME=$CARGO_HOME" >> "$GITHUB_ENV"
    echo "RUSTUP_HOME=$RUSTUP_HOME" >> "$GITHUB_ENV"
  fi
else
  # Bring a preinstalled toolchain up to the newest stable, with the
  # components the checks use. When the update server (or a runner's mirror)
  # cannot be reached, the toolchain the image has will do.
  if ! rustup toolchain install stable --profile minimal --component rustfmt,clippy --no-self-update; then
    echo "Could not update Rust; using $(rustc --version)" >&2
  fi
  rustup default stable >/dev/null
fi
# A self-hosted runner may name a crates.io mirror (CI_CARGO_MIRROR); without
# one, cargo uses crates.io.
if [ -n "${CI_CARGO_MIRROR:-}" ]; then
  mkdir -p "${CARGO_HOME:-$HOME/.cargo}"
  cat > "${CARGO_HOME:-$HOME/.cargo}/config.toml" <<EOF
[source.crates-io]
replace-with = "ci-mirror"

[source.ci-mirror]
registry = "$CI_CARGO_MIRROR"

# Mirrors throttle bursts from parallel jobs; retry with backoff.
[net]
retry = 8
EOF
  echo "crates.io via $CI_CARGO_MIRROR"
fi
if [ $# -gt 0 ]; then
  rustup target add "$@"
fi
rustc --version
cargo --version
