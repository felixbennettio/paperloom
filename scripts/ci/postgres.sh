#!/bin/bash
# Starts a throwaway PostgreSQL for the sync service's tests at
# postgres://ci:ci@127.0.0.1:5432/paperloom_test (run.sh server and the GitLab
# job export it as PAPERLOOM_TEST_DATABASE_URL; so does GITHUB_ENV on GitHub
# Actions). Linux CI only; run linux-deps.sh first.
set -euo pipefail

as_postgres() {
  if [ "$(id -u)" = 0 ]; then su postgres -c "$1"; else sudo -u postgres bash -c "$1"; fi
}
SUDO=""
if [ "$(id -u)" != 0 ]; then SUDO=sudo; fi
export DEBIAN_FRONTEND=noninteractive
$SUDO apt-get install -y -qq --no-install-recommends postgresql >/dev/null
$SUDO service postgresql start
as_postgres "psql -q -c \"CREATE ROLE ci LOGIN SUPERUSER PASSWORD 'ci'\""
as_postgres "createdb -O ci paperloom_test"
if [ -n "${GITHUB_ENV:-}" ]; then
  echo "PAPERLOOM_TEST_DATABASE_URL=postgres://ci:ci@127.0.0.1:5432/paperloom_test?sslmode=disable" >> "$GITHUB_ENV"
fi
echo "PostgreSQL $(as_postgres 'psql -tAc "SHOW server_version"') is up"
