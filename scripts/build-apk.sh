#!/usr/bin/env bash
# Build an installable APK.
#
#   ./scripts/build-apk.sh                     debug APK, every ABI in Cargo.toml
#   ./scripts/build-apk.sh --release           release APK, signed
#   ./scripts/build-apk.sh --abi arm64         just arm64-v8a (much faster)
#   ./scripts/build-apk.sh --release --install install onto the connected device
#   ./scripts/build-apk.sh --release --out dir  also copy the APK into `dir`
#
# Release builds sign with APK_KEYSTORE (the keystore base64-encoded, or a
# path to it) and APK_KEYSTORE_PASSWORD when both are set, so every build
# carries the same signature and installs over the last one. Without them a
# local throwaway key is made.
#
# Run scripts/setup-android.sh once first.
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=scripts/android-env.sh
source scripts/android-env.sh

PROFILE=debug
INSTALL=0
TARGET=""
OUT_DIR=""

while [ $# -gt 0 ]; do
    case "$1" in
        --release)      PROFILE=release ;;
        --debug)        PROFILE=debug ;;
        --install|-i)   INSTALL=1 ;;
        --out)          OUT_DIR="${2:?--out needs a directory}"; shift ;;
        --abi)
            case "${2:-}" in
                arm64|arm64-v8a)        TARGET=aarch64-linux-android ;;
                arm|armeabi-v7a)        TARGET=armv7-linux-androideabi ;;
                x86_64)                 TARGET=x86_64-linux-android ;;
                all)                    TARGET="" ;;
                *) echo "unknown --abi '${2:-}' (arm64|arm|x86_64|all)" >&2; exit 2 ;;
            esac
            shift ;;
        -h|--help)      sed -n '2,15p' "$0" | sed 's/^# \?//'; exit 0 ;;
        *)              echo "unknown option '$1'" >&2; exit 2 ;;
    esac
    shift
done

args=(build --lib)
[ "$PROFILE" = release ] && args+=(--release)
[ -n "$TARGET" ] && args+=(--target "$TARGET")

# Opens the keystore in APK_KEYSTORE and has cargo-apk2 sign with it.
#
# A keystore cannot be handed to a Docker build on Coolify as a file, only as a
# variable, so APK_KEYSTORE is normally the file base64-encoded; a path to the
# file works too, for local builds. It is checked here, before the minutes of
# compiling, rather than by apksigner at the very end.
use_apk_keystore() {
    [ -n "${APK_KEYSTORE_PASSWORD:-}" ] || {
        echo "APK_KEYSTORE is set but APK_KEYSTORE_PASSWORD is not; set both, or neither" >&2
        exit 2
    }
    local keystore
    if [ -f "$APK_KEYSTORE" ]; then
        keystore="$APK_KEYSTORE"
    else
        # Decoded into a private temporary file, removed when the script ends.
        keystore="$(mktemp)"
        chmod 600 "$keystore"
        trap 'rm -f "'"$keystore"'"' EXIT
        if ! tr -d ' \t\r\n' <<<"$APK_KEYSTORE" | base64 -d >"$keystore" 2>/dev/null ||
            [ ! -s "$keystore" ]; then
            echo "APK_KEYSTORE is neither a keystore file nor base64 of one" >&2
            exit 2
        fi
    fi

    local listing
    if ! listing="$(keytool -list -v -keystore "$keystore" \
        -storepass "$APK_KEYSTORE_PASSWORD" 2>&1)"; then
        echo "APK_KEYSTORE does not open with APK_KEYSTORE_PASSWORD:" >&2
        echo "$listing" | head -3 >&2
        exit 2
    fi
    # apksigner is given no alias, so the keystore must hold exactly one key,
    # and that key's password must be the keystore's (keytool's PKCS12 default).
    local keys
    keys="$(grep -c 'PrivateKeyEntry' <<<"$listing" || true)"
    [ "$keys" = 1 ] || {
        echo "APK_KEYSTORE must hold exactly one key; it holds $keys" >&2
        exit 2
    }

    export CARGO_APK_RELEASE_KEYSTORE="$keystore"
    export CARGO_APK_RELEASE_KEYSTORE_PASSWORD="$APK_KEYSTORE_PASSWORD"
    # The fingerprint goes into the log, so two builds can be seen to share a key.
    echo "Signing with the key in APK_KEYSTORE, certificate" \
        "$(grep -m1 -o 'SHA256: .*' <<<"$listing")"
}

if [ "$PROFILE" = release ] && [ -z "${CARGO_APK_RELEASE_KEYSTORE:-}" ] &&
    [ -n "${APK_KEYSTORE:-}" ]; then
    use_apk_keystore
fi

if [ "$PROFILE" = release ]; then
    # `cargo apk2` auto-generates a debug keystore, but refuses to invent one for
    # release builds. Keep a local keystore next to the project so release APKs
    # are reproducibly signed; it is gitignored.
    if [ -z "${CARGO_APK_RELEASE_KEYSTORE:-}" ]; then
        export CARGO_APK_RELEASE_KEYSTORE="$PWD/android/keystore/release.jks"
        export CARGO_APK_RELEASE_KEYSTORE_PASSWORD="${WORDTEE_KEYSTORE_PASSWORD:-android}"

        if [ ! -f "$CARGO_APK_RELEASE_KEYSTORE" ]; then
            echo "Creating a local release keystore at $CARGO_APK_RELEASE_KEYSTORE"
            echo "NOTE: this is a throwaway key for local installs. An APK signed with it"
            echo "      will not install over one signed with any other key; set"
            echo "      APK_KEYSTORE and APK_KEYSTORE_PASSWORD to sign with your own."
            mkdir -p "$(dirname "$CARGO_APK_RELEASE_KEYSTORE")"
            keytool -genkeypair -v \
                -keystore "$CARGO_APK_RELEASE_KEYSTORE" \
                -storepass "$CARGO_APK_RELEASE_KEYSTORE_PASSWORD" \
                -keypass "$CARGO_APK_RELEASE_KEYSTORE_PASSWORD" \
                -alias wordtee \
                -keyalg RSA -keysize 2048 -validity 10000 \
                -dname "CN=wordtee, OU=Dev, O=Example, L=, S=, C=" >/dev/null
        fi
    fi
fi

cargo apk2 "${args[@]}"

APK="$(find "target/$PROFILE/apk" -maxdepth 1 -name '*.apk' ! -name '*-unaligned.apk' |
    head -1)"
[ -n "$APK" ] || { echo "no APK produced" >&2; exit 1; }

echo
echo "APK: $APK ($(du -h "$APK" | cut -f1))"

if [ -n "$OUT_DIR" ]; then
    mkdir -p "$OUT_DIR"
    cp "$APK" "$OUT_DIR/"
    echo "Copied to $OUT_DIR/$(basename "$APK")"
fi

if [ "$INSTALL" = 1 ]; then
    echo "Installing onto the connected device..."
    adb install -r "$APK"
    adb shell monkey -p com.example.wordtee -c android.intent.category.LAUNCHER 1 >/dev/null
fi
