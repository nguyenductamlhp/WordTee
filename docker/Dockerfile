# Builds the Android APK and the WebAssembly demo, then serves both from nginx.
#
#   docker compose build                         builds everything
#   docker run --rm -p 8080:80 wordtee-web        →  http://localhost:8080
#
# The Android SDK/NDK and the Rust toolchains live only in the build stages. The
# image that actually ships is nginx plus static files, so it stays small.

ARG RUST_VERSION=1.98.1
ARG TRUNK_VERSION=0.21.14
ARG NGINX_VERSION=1.31

# ---------------------------------------------------------------------------
# Stage 1 — the APK.
#
# Pinned to amd64 on purpose: Google publishes the SDK build-tools and the NDK
# host toolchain for linux-x86_64 only. On an arm64 host (Apple silicon) this
# stage runs under emulation, which works but is slow.
# ---------------------------------------------------------------------------
FROM --platform=linux/amd64 rust:${RUST_VERSION}-bookworm AS apk-builder

# arm64 | arm | x86_64 | all
ARG APK_ABI=arm64

ENV ANDROID_HOME=/opt/android-sdk \
    DEBIAN_FRONTEND=noninteractive

# The Android build tools are Java programs; `unzip` is used by the SDK setup.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        openjdk-17-jdk-headless \
        unzip \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /src

# Toolchain first, source second: editing the app must not re-download ~2 GB of
# SDK and NDK. This is the same script a developer runs on their own machine.
COPY scripts/ ./scripts/
RUN chmod +x ./scripts/*.sh \
 && ./scripts/setup-android.sh

COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY android/ ./android/
# The dictionary is `include_bytes!`d into the binary, so it has to be here
# before the compiler runs. See scripts/build-dict.py.
COPY assets/ ./assets/

# `--out /out` leaves a directory holding just the finished APK, with no
# intermediate unaligned copy for the web server to serve by accident.
RUN ./scripts/build-apk.sh --release --abi "${APK_ABI:-arm64}" --out /out \
 && ./scripts/apk-info.sh /out/*.apk > /out/build-info.json \
 && cat /out/build-info.json

# ---------------------------------------------------------------------------
# Stage 2 — the WebAssembly build.
#
# `trunk` downloads matching wasm-bindgen and wasm-opt binaries itself, so this
# stage needs network access at build time.
# ---------------------------------------------------------------------------
FROM --platform=linux/amd64 rust:${RUST_VERSION}-bookworm AS web-builder

ARG TRUNK_VERSION

WORKDIR /src

RUN rustup target add wasm32-unknown-unknown \
 && curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz" \
      | tar xz -C /usr/local/bin trunk \
 && trunk --version

COPY Cargo.toml Cargo.lock Trunk.toml ./
COPY src/ ./src/
COPY web/ ./web/
COPY assets/ ./assets/

RUN trunk build --release

# ---------------------------------------------------------------------------
# Stage 3 — what gets deployed. `/download/` holds the APK and the JSON the
# page reads to describe it.
# ---------------------------------------------------------------------------
FROM nginx:${NGINX_VERSION}-alpine AS runtime

COPY docker/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=web-builder /src/dist/ /usr/share/nginx/html/
COPY --from=apk-builder /out/ /usr/share/nginx/html/download/

EXPOSE 80
