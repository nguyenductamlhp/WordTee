#!/usr/bin/env bash
# Locate the JDK, Android SDK and NDK needed to package an APK.
#
# Source this file (`source scripts/android-env.sh`) to get ANDROID_HOME,
# ANDROID_NDK_ROOT, JAVA_HOME and PATH set for the current shell. Anything you
# have already exported wins, so an existing Android Studio install is used as-is.

_fail() { echo "error: $*" >&2; return 1; }

# --- SDK -------------------------------------------------------------------
ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
[ -d "$ANDROID_HOME" ] || _fail "Android SDK not found at $ANDROID_HOME (run scripts/setup-android.sh)" || return 1
ANDROID_SDK_ROOT="$ANDROID_HOME"
export ANDROID_HOME ANDROID_SDK_ROOT

# --- NDK -------------------------------------------------------------------
if [ -z "${ANDROID_NDK_ROOT:-}" ]; then
    # Highest installed side-by-side NDK. `sort -V` orders 27.3.x above 27.10.x
    # correctly, unlike a plain lexicographic sort.
    ANDROID_NDK_ROOT="$(find "$ANDROID_HOME/ndk" -maxdepth 1 -mindepth 1 -type d 2>/dev/null |
        sort -V | tail -1)"
fi
[ -n "$ANDROID_NDK_ROOT" ] && [ -d "$ANDROID_NDK_ROOT" ] ||
    _fail "no NDK under $ANDROID_HOME/ndk (run scripts/setup-android.sh)" || return 1
ANDROID_NDK_HOME="$ANDROID_NDK_ROOT"
export ANDROID_NDK_ROOT ANDROID_NDK_HOME

# --- JDK -------------------------------------------------------------------
# Needed by aapt2/apksigner/keytool, which are Java tools.
if [ -z "${JAVA_HOME:-}" ]; then
    if [ -x "$HOME/Android/jdk/bin/java" ]; then
        JAVA_HOME="$HOME/Android/jdk"
    elif command -v java >/dev/null 2>&1; then
        JAVA_HOME="$(dirname "$(dirname "$(readlink -f "$(command -v java)")")")"
    fi
fi
[ -n "${JAVA_HOME:-}" ] && [ -x "$JAVA_HOME/bin/java" ] ||
    _fail "no JDK found (run scripts/setup-android.sh, or set JAVA_HOME)" || return 1
export JAVA_HOME
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"

unset -f _fail
