#!/bin/bash
# Points the generated Android project at the runner's mirrors: the Maven
# mirror through a Gradle init script, the Gradle distribution through the
# wrapper's download URL. Does nothing when the runner names no mirrors.
#
#   scripts/ci/android-mirrors.sh <android project dir>
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PROJECT=${1:?android project dir}

if [ -n "${CI_MAVEN_MIRROR:-}" ]; then
  INIT_DIR="${GRADLE_USER_HOME:-$HOME/.gradle}/init.d"
  mkdir -p "$INIT_DIR"
  cp "$ROOT/ci/gradle/mirrors.init.gradle" "$INIT_DIR/ci-mirrors.gradle"
  echo "Maven via $CI_MAVEN_MIRROR"
fi
WRAPPER="$PROJECT/gradle/wrapper/gradle-wrapper.properties"
if [ -n "${CI_GRADLE_DIST_MIRROR:-}" ] && [ -f "$WRAPPER" ]; then
  sed -i -E "s#^distributionUrl=.*/(gradle-[^/]+\\.zip)\$#distributionUrl=${CI_GRADLE_DIST_MIRROR%/}/\\1#" "$WRAPPER"
  grep '^distributionUrl=' "$WRAPPER"
fi
