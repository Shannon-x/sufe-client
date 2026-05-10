#!/usr/bin/env bash
# One-time bootstrap for the Gradle wrapper.
#
# We don't check in `gradle-wrapper.jar` (binary noise in version control)
# nor pre-generated `gradlew` shell scripts (they're upstream boilerplate).
# Run this once after a fresh clone, then use `./gradlew` as normal.
#
# `just android-bootstrap` invokes this — see ../justfile.

set -euo pipefail

cd "$(dirname "$0")"

GRADLE_TAG="${GRADLE_TAG:-v8.7.0}"
RAW_BASE="https://raw.githubusercontent.com/gradle/gradle/${GRADLE_TAG}"

WRAPPER_JAR="gradle/wrapper/gradle-wrapper.jar"

if [ ! -f "${WRAPPER_JAR}" ]; then
    echo "→ Fetching ${WRAPPER_JAR} (${GRADLE_TAG})"
    mkdir -p "$(dirname "${WRAPPER_JAR}")"
    curl -fsSL -o "${WRAPPER_JAR}" "${RAW_BASE}/gradle/wrapper/gradle-wrapper.jar"
fi

if [ ! -f gradlew ]; then
    echo "→ Fetching gradlew"
    curl -fsSL -o gradlew "${RAW_BASE}/gradlew"
    chmod +x gradlew
fi

if [ ! -f gradlew.bat ]; then
    echo "→ Fetching gradlew.bat"
    curl -fsSL -o gradlew.bat "${RAW_BASE}/gradlew.bat"
fi

echo
echo "Done. Try:"
echo "  ./gradlew --version"
echo "  ./gradlew :app:assembleDebug"
