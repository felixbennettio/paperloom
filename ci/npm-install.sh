#!/usr/bin/env bash
# Run from app/. Keep the runner's registry and downloaded tarball cache.
set -eu

# These settings apply only to this process and its npm invocations.
export npm_config_fetch_retries=5
export npm_config_fetch_retry_mintimeout=10000
export npm_config_fetch_retry_maxtimeout=120000

command=install
if [ -f package-lock.json ]; then
  command=ci
fi

for attempt in 1 2 3; do
  echo "npm $command: attempt $attempt/3"
  if npm "$command" --no-audit --no-fund; then
    echo "npm $command succeeded on attempt $attempt/3"
    exit 0
  else
    status=$?
  fi

  if [ "$attempt" -eq 3 ]; then
    echo "npm $command failed after 3 attempts (exit $status)" >&2
    exit "$status"
  fi

  case "$attempt" in
    1) delay=15 ;;
    2) delay=45 ;;
  esac
  echo "npm $command attempt $attempt failed (exit $status); retrying in ${delay}s"
  sleep "$delay"
done
