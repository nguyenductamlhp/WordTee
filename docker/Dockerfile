# Builds the Android APK and the WebAssembly demo, then serves both from nginx.
#
#   docker compose build                         builds everything
#   docker run --rm -p 8080:80 \
#       -e BASIC_AUTH_USER=me -e BASIC_AUTH_PASSWORD=… \
#       wordtee-web                              →  http://localhost:8080
#
# The Android SDK/NDK and the Rust toolchains live only in the build stages. The
# image that actually ships is nginx plus static files, so it stays small.
#
# The build runs on the deployment server, so every tool it downloads is pinned
# and checked: by checksum here, or by Cargo.lock for anything from crates.io.

ARG RUST_VERSION=1.98.1
ARG NGINX_VERSION=1.31

# Bump each version together with its checksum: the release publishes it next
# to the archive, as `<archive>.sha256`.
ARG TRUNK_VERSION=0.21.14
ARG TRUNK_SHA256=f2b4680cd239693a646a2795e4633c625328d7b2a044fbe749fa3a2fe9e7036b
ARG BINARYEN_VERSION=123
ARG BINARYEN_SHA256=e959f2170af4c20c552e9de3a0253704d6a9d2766e8fdb88e4d6ac4bae9388fe

# Must equal the `wasm-bindgen` version in Cargo.lock. If they drift apart the
# build stops, naming both versions.
ARG WASM_BINDGEN_VERSION=0.2.128

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

# Google sign-in, compiled into the app; empty builds it without. Declared
# here, after the SDK layers, so changing them rebuilds only the app.
ARG WORDTEE_GOOGLE_DESKTOP_CLIENT_ID=
ARG WORDTEE_GOOGLE_DESKTOP_CLIENT_SECRET=

# `--out /out` leaves a directory holding just the finished APK, with no
# intermediate unaligned copy for the web server to serve by accident.
RUN ./scripts/build-apk.sh --release --abi "${APK_ABI:-arm64}" --out /out \
 && ./scripts/apk-info.sh /out/*.apk > /out/build-info.json \
 && cat /out/build-info.json

# ---------------------------------------------------------------------------
# Stage 2 — the WebAssembly build.
#
# Left to itself, `trunk` downloads wasm-bindgen and wasm-opt without checking
# them. Instead both are installed here, and `--offline` makes trunk use them
# or fail, never fetch its own.
# ---------------------------------------------------------------------------
FROM --platform=linux/amd64 rust:${RUST_VERSION}-bookworm AS web-builder

ARG TRUNK_VERSION
ARG TRUNK_SHA256
ARG BINARYEN_VERSION
ARG BINARYEN_SHA256
ARG WASM_BINDGEN_VERSION

WORKDIR /src

RUN rustup target add wasm32-unknown-unknown \
 && curl -fsSL -o /tmp/trunk.tar.gz \
      "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz" \
 && echo "${TRUNK_SHA256}  /tmp/trunk.tar.gz" | sha256sum -c - \
 && tar xzf /tmp/trunk.tar.gz -C /usr/local/bin trunk \
 && curl -fsSL -o /tmp/binaryen.tar.gz \
      "https://github.com/WebAssembly/binaryen/releases/download/version_${BINARYEN_VERSION}/binaryen-version_${BINARYEN_VERSION}-x86_64-linux.tar.gz" \
 && echo "${BINARYEN_SHA256}  /tmp/binaryen.tar.gz" | sha256sum -c - \
 && tar xzf /tmp/binaryen.tar.gz -C /usr/local/bin --strip-components=2 \
      "binaryen-version_${BINARYEN_VERSION}/bin/wasm-opt" \
 && rm /tmp/trunk.tar.gz /tmp/binaryen.tar.gz \
 && cargo install wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked \
 && trunk --version && wasm-opt --version && wasm-bindgen --version

COPY Cargo.toml Cargo.lock Trunk.toml ./
COPY src/ ./src/
COPY web/ ./web/
COPY assets/ ./assets/

# Google sign-in, compiled into the app; empty builds it without.
ARG WORDTEE_GOOGLE_WEB_CLIENT_ID=

# `--offline` reaches cargo too, so the crates are fetched first, checked
# against Cargo.lock.
RUN cargo fetch --locked \
 && trunk build --release --offline

# ---------------------------------------------------------------------------
# Stage 3 — what gets deployed. `/download/` holds the APK and the JSON the
# page reads to describe it.
#
# The unprivileged nginx image runs as uid 101 and keeps its pid and temp files
# in /tmp, so the container needs neither root nor a writable filesystem
# (docker-compose.yml takes both away).
# ---------------------------------------------------------------------------
FROM nginxinc/nginx-unprivileged:${NGINX_VERSION}-alpine AS runtime

COPY docker/nginx.conf /etc/nginx/conf.d/default.conf
COPY docker/security-headers.conf /etc/nginx/snippets/security-headers.conf
COPY --chmod=755 docker/40-basic-auth.sh /docker-entrypoint.d/40-basic-auth.sh
COPY --from=web-builder /src/dist/ /usr/share/nginx/html/
COPY --from=apk-builder /out/ /usr/share/nginx/html/download/

EXPOSE 80
