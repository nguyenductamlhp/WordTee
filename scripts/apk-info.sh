#!/usr/bin/env bash
# Print a JSON description of an APK on stdout.
#
#   ./scripts/apk-info.sh target/release/apk/tap-counter.apk > build-info.json
#
# The download page (web/index.html) fetches this at runtime to fill in the
# version, size, ABIs and checksum, so the page itself stays build-agnostic.
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=scripts/android-env.sh
source scripts/android-env.sh

APK="${1:-}"
[ -f "$APK" ] || { echo "usage: $0 <path-to.apk>" >&2; exit 2; }

AAPT2="$(find "$ANDROID_HOME/build-tools" -name aapt2 -type f | sort -V | tail -1)"
[ -x "$AAPT2" ] || { echo "aapt2 not found under $ANDROID_HOME/build-tools" >&2; exit 1; }

badging="$("$AAPT2" dump badging "$APK")"

# `package:` is one long line holding several `…name='…'` pairs, so every
# pattern below is anchored: an unanchored `.*name='` would greedily match the
# last pair on the line rather than the one we want.
pick() { sed -n "s/$1/\1/p" <<<"$badging" | head -1; }

package="$(pick "^package: name='\([^']*\)'.*")"
version="$(pick "^package: .*[[:space:]]versionName='\([^']*\)'.*")"
min_sdk="$(pick "^minSdkVersion:'\([0-9]*\)'.*")"
target_sdk="$(pick "^targetSdkVersion:'\([0-9]*\)'.*")"

for required in package version min_sdk target_sdk; do
    [ -n "${!required}" ] || { echo "could not read $required from aapt2 badging" >&2; exit 1; }
done

# Marketing version for the API level, so the page can say "Android 7.0" rather
# than "API 24". Only the levels this project might plausibly target.
case "$min_sdk" in
    21) min_android="5.0" ;; 22) min_android="5.1" ;; 23) min_android="6.0" ;;
    24) min_android="7.0" ;; 25) min_android="7.1" ;; 26) min_android="8.0" ;;
    27) min_android="8.1" ;; 28) min_android="9" ;;   29) min_android="10" ;;
    30) min_android="11" ;;  31|32) min_android="12" ;; 33) min_android="13" ;;
    34) min_android="14" ;;  35) min_android="15" ;;  36) min_android="16" ;;
    *)  min_android="API $min_sdk" ;;
esac

# ABIs are whichever lib/<abi>/ directories the APK actually contains.
abis="$(unzip -Z1 "$APK" 'lib/*' 2>/dev/null | cut -d/ -f2 | sort -u |
    sed 's/.*/"&"/' | paste -sd, -)"

cat <<JSON
{
  "apk": "$(basename "$APK")",
  "version": "$version",
  "package": "${package}",
  "size_bytes": $(stat -c %s "$APK"),
  "sha256": "$(sha256sum "$APK" | cut -d' ' -f1)",
  "abis": [${abis}],
  "min_sdk": ${min_sdk},
  "min_android": "${min_android}",
  "target_sdk": ${target_sdk},
  "built_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
JSON
