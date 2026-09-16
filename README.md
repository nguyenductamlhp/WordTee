# Tap Counter

A tiny [egui](https://github.com/emilk/egui) app: tap (or click) anywhere on the
screen and the counter goes up. The same Rust code runs as a native desktop
binary, as an installable Android APK, and as WebAssembly in a browser.

```
┌──────────────────────────────┐
│ Tap Counter    7 taps [Reset]│
├──────────────────────────────┤
│                              │
│             ╭───╮            │
│          ╭──┤ 7 ├──╮         │   ← tap anywhere in this area
│          ╰──┴───┴──╯         │     (ripple shows where you hit)
│         tap anywhere         │
│                              │
└──────────────────────────────┘
```

- Every press inside the main area counts, right out to the screen edges.
- A ripple expands from the touch point, and the number pops, so a tap is
  visible as well as counted.
- <kbd>Space</kbd> / <kbd>Enter</kbd> also count, for desktop and hardware
  keyboards.
- **Reset** clears the count.

## Docker

One build produces the APK *and* the browser version, and one image serves a
page that offers both — a live demo you can tap, and a download link for the APK
it just built:

```
┌───────────────────────────────────────────────────┐
│  ⬤ Tap Counter                                    │
│                                                   │
│   ┌─────────────┐    Install on Android           │
│   │  ╭───────╮  │    ┌───────────────────────┐    │
│   │  │   7   │  │    │ ⬇ Download APK · 2.7MB│    │
│   │  ╰───────╯  │    └───────────────────────┘    │
│   │  live demo  │    Version   0.1.0              │
│   │  (wasm)     │    ABIs      arm64-v8a          │
│   └─────────────┘    Requires  Android 7.0+       │
│    tap it →          SHA-256   …                  │
└───────────────────────────────────────────────────┘
```

The page reads the APK's real version, size, ABIs and SHA-256 at runtime from
`/download/build-info.json`, which the build generates — nothing is hardcoded.
On a phone the download card comes first, since that is what you are there for.

`docker-compose.yml` is written for [Coolify](#deploy-to-coolify), which means
it deliberately publishes no host port: Coolify's proxy reaches the container
over the Docker network. Locally, publish one yourself:

```sh
docker compose build                            # → image tap-counter-web
docker run --rm -p 8080:80 tap-counter-web      # → http://localhost:8080
```

| Command | Effect |
| --- | --- |
| `docker compose build` | Build the APK, the wasm bundle and the image |
| `APK_ABI=all docker compose build` | …with an APK for all three ABIs |
| `docker run --rm -p 8080:80 tap-counter-web` | Serve it on `localhost:8080` |
| `docker run --rm -v "$PWD/out:/out" tap-counter-web cp /usr/share/nginx/html/download/tap-counter.apk /out/` | Copy the APK onto the host |

**Expect the first build to take a while**: it downloads the Android SDK and NDK
(~2.4 GB) and compiles egui twice, once for Android and once for WebAssembly.
Later builds reuse those layers unless `scripts/` changes. Only nginx and static
files end up in the final image; the toolchains stay in the build stages.

Two things worth knowing:

- The Android build stage is pinned to `linux/amd64`, because Google ships the
  SDK build-tools and the NDK host toolchain for x86-64 Linux only. On Apple
  silicon it runs under emulation — correct, but slow.
- Each image build generates a fresh throwaway signing key, so two APKs from two
  builds have different signatures. Android refuses to install one over the
  other; uninstall first, or supply your own keystore (see
  [Signing](#signing)).

## Deploy to Coolify

In [Coolify](https://coolify.io): **+ New Resource → your Git repository →
Docker Compose**, then

| Setting | Value |
| --- | --- |
| Base Directory | `/` |
| Docker Compose Location | `/docker-compose.yml` |

Then put your address in the `web` service's **Domains** field, written with the
container port: `https://apk.example.com:80`. The `:80` suffix tells Coolify's
proxy which port inside the container to talk to; the public site still answers
on 80/443.

Deploy. Coolify clones the repo, builds the APK and the WebAssembly bundle on
the server, and serves the page on that domain.

To build for all three ABIs, add `APK_ABI=all` under **Environment Variables**.
Coolify's separate *Build Variables* do not reach Compose builds, which is why
the compose file reads it as `${APK_ABI:-arm64}`.

Three things in the compose file exist because of Coolify, and are worth not
undoing by accident:

- **No `ports:`.** A published host port bypasses Coolify's proxy, and
  `coolify-proxy` already binds 80, 443 and 8080, so the obvious mapping would
  collide on deploy.
- **No `environment:` block.** Coolify's docs suggest a bare `SERVICE_URL_WEB_80`
  entry to get a generated domain, but on a Git-based Compose app that breaks
  the deploy: Coolify rewrites the `environment` list into a map by splitting
  each entry on `=`, and an entry without one keeps its list index, emitting
  `0: SERVICE_URL_WEB_80` — which Compose rejects with *non-string key in
  services.web.environment: 0* ([#5064](https://github.com/coollabsio/coolify/issues/5064)).
  Writing it as `- SERVICE_URL_WEB_80=`, with the trailing equals sign, avoids
  that; the compose file has it commented out. Setting the domain by hand costs
  one field and sidesteps the whole area.
- **One service, no `profiles:`.** Coolify does not honour Compose profiles
  ([#6395](https://github.com/coollabsio/coolify/issues/6395)), so a
  local-only helper service in this file would get deployed too. That is why
  exporting the APK is a `docker run` above rather than a second service.

### What the server needs

The build is the demanding part; what it leaves running is not.

| | |
| --- | --- |
| Disk during build | ~10 GB (Android SDK + NDK 2.4 GB, Cargo registry 0.7 GB, build output ~1.6 GB, base images) |
| RAM | 4 GB is comfortable; linking is the peak |
| First deploy | Slow — it downloads ~2.4 GB of toolchain and compiles egui twice |
| Running image | nginx + ~7 MB of static files |
| Rebuilds | Fast unless `scripts/` changes, which invalidates the SDK layer |

If the server is tight on disk, `docker builder prune` between deploys reclaims
the build cache. If it is tight enough that the build itself will not fit,
building the image in CI and pointing the compose file at a registry image is
the better shape — ask and I can add that.

### Troubleshooting

- **`COPY` fails with "no such file or directory"** — the build context did not
  reach the builder. Check that Base Directory is `/`. This has been reported
  against some Coolify betas ([#6002](https://github.com/coollabsio/coolify/issues/6002)).
- **Site loads but the APK button says "APK not available"** — the Android stage
  produced nothing. The container's `/usr/share/nginx/html/download/` should
  hold `tap-counter.apk` and `build-info.json`; the healthcheck fails too in
  that case, so Coolify will mark the service unhealthy.
- **Build times out** — raise the deployment timeout in Coolify, or set
  `APK_ABI=arm64` (the default) rather than `all`.

## Layout

| Path | What it is |
| --- | --- |
| `src/app.rs` | The app itself — `TapCounterApp` — plus its tests |
| `src/main.rs` | `main`, for desktop and (compiled to wasm) for the browser |
| `src/android.rs` | `android_main`, the entry point Android calls |
| `src/web.rs` | Boots the app into a `<canvas>` |
| `web/` | The download / demo page that Trunk turns into `dist/` |
| `android/res/` | Launcher icon resources compiled into the APK |
| `docker/` | `Dockerfile` (3 stages) and the nginx config |
| `docker-compose.yml` | The deployment: builds everything, serves it on port 80 |
| `scripts/setup-android.sh` | One-time install of the JDK, Android SDK, NDK and `cargo-apk2` |
| `scripts/build-apk.sh` | Builds, exports and optionally installs the APK |
| `scripts/apk-info.sh` | Emits the `build-info.json` the download page reads |
| `scripts/android-env.sh` | Sourceable env setup, used by the other scripts |

The Android packaging is configured under `[package.metadata.android]` in
`Cargo.toml` — there is no Gradle project and no Java or Kotlin source. The APK
uses Android's built-in `NativeActivity`, which loads `libtcheckee.so` and calls
`android_main`.

## Desktop

```sh
cargo run            # debug
cargo run --release
cargo test           # headless tests for the tap logic
```

## Android

### 1. Install the toolchain (once)

```sh
./scripts/setup-android.sh
```

This needs no root and touches nothing outside your home directory. It installs:

- Temurin JDK 17 into `~/Android/jdk` — *skipped if you already have a JDK*
- The Android SDK, platform 35, build-tools 35.0.1 and NDK 27.3 into
  `~/Android/Sdk` (~2 GB; set `ANDROID_HOME` first to put it elsewhere)
- The `aarch64`/`armv7`/`x86_64` Android Rust targets
- [`cargo-apk2`](https://crates.io/crates/cargo-apk2), which does the packaging

Already have Android Studio? Export `ANDROID_HOME` and `JAVA_HOME` and the
script will reuse them, only filling in what is missing.

### 2. Build

```sh
./scripts/build-apk.sh                        # debug APK, all three ABIs
./scripts/build-apk.sh --release              # release APK, signed  (~8 MB)
./scripts/build-apk.sh --release --abi arm64  # one ABI: much faster  (~2.7 MB)
./scripts/build-apk.sh --release --install    # ...and push it to a connected device
./scripts/build-apk.sh --release --out ./out  # ...and copy it somewhere
```

The APK lands in `target/release/apk/tap-counter.apk` (or `target/debug/apk/…`).
Copy it to a phone and open it, or use `adb install -r <path>`.

`--abi` takes `arm64`, `arm`, `x86_64` or `all`. Real phones are `arm64`;
`x86_64` is what emulators usually need.

### Signing

Debug builds are signed with the standard Android debug key
(`~/.android/debug.keystore`), auto-created on first use.

Release builds need a real key. `build-apk.sh` generates a throwaway one at
`android/keystore/release.jks` (gitignored) so you can install release builds
immediately. **Before publishing anywhere, make your own keystore and back it
up** — an app store ties your app's identity to that key forever:

```sh
keytool -genkeypair -v -keystore ~/my-release.jks -alias tapcounter \
        -keyalg RSA -keysize 2048 -validity 10000

export CARGO_APK_RELEASE_KEYSTORE=~/my-release.jks
export CARGO_APK_RELEASE_KEYSTORE_PASSWORD=...
./scripts/build-apk.sh --release
```

`build-apk.sh` leaves an externally-set `CARGO_APK_RELEASE_KEYSTORE` alone.

### Changing app identity

Everything lives in `[package.metadata.android]` in `Cargo.toml`:
`package` (`com.example.tcheckee`), `label`, `apk_name`, `min_sdk_version` /
`target_sdk_version`, and the ABI list in `build_targets`.

### Logs from a device

```sh
adb logcat -s tcheckee:V RustStdoutStderr:V
```

## Web

Docker does this for you; to iterate on the page itself, run
[Trunk](https://trunkrs.dev) directly:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk           # or grab a prebuilt binary from the releases page

trunk serve                   # http://127.0.0.1:8080, rebuilds on save
trunk build --release         # → dist/
```

`dist/` is what nginx serves. The download section degrades gracefully: with no
`dist/download/build-info.json` it says so and disables the button, so
`trunk serve` works without building an APK first. To see the full page locally:

```sh
./scripts/build-apk.sh --release --abi arm64 --out dist/download
./scripts/apk-info.sh dist/download/tap-counter.apk > dist/download/build-info.json
```

## Notes

- The renderer is `glow` — OpenGL ES on Android, WebGL 2 in the browser, OpenGL
  on desktop. It is the lightest and most portable eframe backend.
- `min_sdk_version` is 24 (Android 7.0).
- `include_cplusplus_shared` is off: none of the built `.so` files actually link
  `libc++_shared`, so there is no reason to ship it. Turn it back on in
  `Cargo.toml` if you add a dependency that needs the C++ runtime.
- eframe's `accesskit` feature is off on Android (incompatible with
  `android-native-activity`) and on the web (it is a native-only feature).
  Desktop builds keep it.
- `wasm-opt` runs with `--enable-bulk-memory-opt`, which current rustc output
  requires and Binaryen does not assume by default.
