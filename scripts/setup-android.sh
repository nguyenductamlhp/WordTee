#!/usr/bin/env bash
# One-time setup of everything needed to turn this crate into an APK:
# a JDK, the Android SDK + NDK, the Rust Android targets and `cargo-apk2`.
#
# Nothing is installed system-wide and no root is needed: the SDK lands in
# $ANDROID_HOME (default ~/Android/Sdk) and, if you have no JDK, one is
# unpacked into ~/Android/jdk.
#
# Re-running is safe; already-installed pieces are skipped.
set -euo pipefail

ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
JDK_DIR="${JDK_DIR:-$HOME/Android/jdk}"

# Pin the SDK pieces so a build is reproducible. Bump them deliberately.
CMDLINE_TOOLS_REV="${CMDLINE_TOOLS_REV:-13114758}"
PLATFORM_VERSION="${PLATFORM_VERSION:-35}"
BUILD_TOOLS_VERSION="${BUILD_TOOLS_VERSION:-35.0.1}"
NDK_VERSION="${NDK_VERSION:-27.3.13750724}"

RUST_TARGETS=(aarch64-linux-android armv7-linux-androideabi x86_64-linux-android)

step() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }

# --- JDK -------------------------------------------------------------------
if [ -n "${JAVA_HOME:-}" ] && [ -x "$JAVA_HOME/bin/java" ]; then
    step "Using the JDK already at \$JAVA_HOME ($JAVA_HOME)"
elif command -v javac >/dev/null 2>&1; then
    step "Using the JDK already on \$PATH ($(command -v javac))"
elif [ -x "$JDK_DIR/bin/java" ]; then
    step "Using the JDK from a previous run ($JDK_DIR)"
else
    step "Installing Temurin JDK 17 into $JDK_DIR"
    mkdir -p "$JDK_DIR"
    curl -fsSL "https://api.adoptium.net/v3/binary/latest/17/ga/linux/x64/jdk/hotspot/normal/eclipse" |
        tar xz -C "$JDK_DIR" --strip-components=1
fi
export JAVA_HOME="${JAVA_HOME:-$([ -x "$JDK_DIR/bin/java" ] && echo "$JDK_DIR" ||
    dirname "$(dirname "$(readlink -f "$(command -v java)")")")}"
export PATH="$JAVA_HOME/bin:$PATH"

# --- Android command-line tools --------------------------------------------
SDKMANAGER="$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager"
if [ ! -x "$SDKMANAGER" ]; then
    step "Installing Android command-line tools into $ANDROID_HOME"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    curl -fsSL -o "$tmp/tools.zip" \
        "https://dl.google.com/android/repository/commandlinetools-linux-${CMDLINE_TOOLS_REV}_latest.zip"
    unzip -q "$tmp/tools.zip" -d "$tmp"
    mkdir -p "$ANDROID_HOME/cmdline-tools"
    # The zip unpacks to `cmdline-tools/`; sdkmanager insists on living in a
    # version directory ("latest") beneath `cmdline-tools/`.
    rm -rf "$ANDROID_HOME/cmdline-tools/latest"
    mv "$tmp/cmdline-tools" "$ANDROID_HOME/cmdline-tools/latest"
else
    step "Android command-line tools already installed"
fi

step "Installing SDK packages (this downloads ~2 GB the first time)"
# sdkmanager asks to accept each licence on stdin. Feeding it `yes` would work,
# but `yes` then dies of SIGPIPE when sdkmanager exits, and `pipefail` turns that
# into a failed script. A bounded here-string is a redirect, not a pipeline, so
# there is nothing left writing when sdkmanager finishes.
CONFIRMATIONS="$(printf 'y\n%.0s' {1..100})"
"$SDKMANAGER" --sdk_root="$ANDROID_HOME" --install \
    "platform-tools" \
    "platforms;android-${PLATFORM_VERSION}" \
    "build-tools;${BUILD_TOOLS_VERSION}" \
    "ndk;${NDK_VERSION}" <<<"$CONFIRMATIONS"

# --- Rust ------------------------------------------------------------------
step "Adding Rust Android targets"
rustup target add "${RUST_TARGETS[@]}"

if command -v cargo-apk2 >/dev/null 2>&1; then
    step "cargo-apk2 already installed ($(cargo apk2 --version))"
else
    step "Installing cargo-apk2"
    cargo install cargo-apk2 --locked
fi

cat <<MSG

Done. Build an APK with:

    ./scripts/build-apk.sh            # debug, all ABIs
    ./scripts/build-apk.sh --release  # release, signed with a local keystore

To use the toolchain from your own shell:

    source scripts/android-env.sh
MSG
