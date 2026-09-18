# WordTee

A Vietnamese-English vocabulary app: look a word up, and the app turns the
lookup into something you are learning. It implements the WordTee v2 spec
(`WordTee.pdf`) — tiered search, sense-by-sense learning items, an adaptive
placement test, FSRS review scheduling and a knowledge map.

Everything is offline. The dictionary ships inside the binary, so there is no
server, no account and no network call at any point. The same Rust code runs as
a native desktop binary, as an Android APK, and as WebAssembly in a browser.

The interface is in English; the meanings are Vietnamese, which is what the
dictionary holds. The bottom bar is four icons over captions — Study, Map,
Look up, You — drawn rather than typed, for the reason in the next section.

```
┌────────────────────────────────┐
│ WordTee            10 due · 1d │
├────────────────────────────────┤
│ swimming                       │
├────────────────────────────────┤
│ swim  /ˈswɪm/   inflected form │
│ “swimming” is the present      │
│ participle / gerund of this    │
│ ────────────────────────────── │
│ 1. [v.] Learning · Core #2,471 │
│    Bơi, lội.                   │
│    to swim across a river      │
├────────────────────────────────┤
│ [I know] [Quick test] [Learn]  │
├────────────────────────────────┤
│  cap    grid    lens    bust   │
│ Study   Map   Look up   You    │
└────────────────────────────────┘
```

## What it does

**Search (spec 1.1).** Four match tiers, ranked by
`tier → edit distance → personal boost → frequency`, never alphabetically:

| Tier | Example |
| --- | --- |
| T1 exact | `hello` → hello |
| T2 inflection | `swimming` → swim · `saw` → saw *and* see · `leaves` → leaf *and* leave |
| T3 prefix | `app` → appear, apply, approach … *then* apposition |
| T4 fuzzy | `teh` → the · `fonetic` → phonetic (only when T1–T3 find fewer than 5) |

Tier T2 draws on three things: the ~21.000 inflections the source dictionary
spells out, suffix rules for the regular cases it does not, and a table of 201
irregular forms — `made`, `came`, `found`, `ran`, `felt` are all in the
thousand most frequent English words and none of them is reachable any other
way.

An exact hit is never displaced by a lemma, so `news` and `glasses` keep their
own entry, with an "also the plural of …" line underneath. Typing Vietnamese with
diacritics searches the definitions instead (`quyết định` → decide, decision).
Worst case measured at 30 ms against the spec's 50 ms budget, and a keystroke
while typing costs 6 ms — `cargo run --release --example bench` prints the lot.

**The word page (spec 1.3, 1.4).** One card per sense, each with its own state,
because spec 1.2 treats a *sense* as the unit of learning, not a word. Three
buttons act on the sense you are looking at: **I know this**, **Quick test**
and **Learn this**, each with a five-second Undo. Quick Test asks two questions,
and the gap-fill never re-uses a sentence already on screen.

**Placement (spec 2.2).** An adaptive test over a Rasch/IRT
model rather than a binary search, because a binary search cannot recover from
one lucky guess. θ and item difficulty share one scale, `ln(rank)`, so the
frontier follows in closed form. Every fifth question is an invented word that
is not within two edits of any real one; claiming to know those is measured as a
false-alarm rate and corrected for, and a user who guesses at everything gets no
frontier at all.

**Study (spec 2.3, 3.x).** Smart Feeding scores unexplored items in a
500-wide window above the frontier by `RankScore × Relevance`, and a session
never contains two senses of one word or a synonym/antonym pair. Reviews are
scheduled by FSRS-5 with a retention target you can set between 0,8 and 0,95;
you never grade yourself, the grade comes from how the exercise went. Exercises
climb three levels as a card stabilises: recognise → recall → produce.

**Map (spec 2.3).** 25 blocks of 1,000 items, each with the spec's four-colour
bar. Inferred progress is hatched so it never reads as confirmed.

**Icons.** The navigation bar's four icons are painted with egui's own shapes:
a mortarboard, a grid of blocks that mirrors the knowledge map, a magnifier and
a bust. Every character the bar tried before this came from a fallback font in a
different typeface, and the map glyph existed in only the crudest of them.
Shapes take about as many lines, always match the text colour beside them, and
cannot go missing. The caption under each is painted too, so it is one centred
line at any width — the labels used to wrap and buckle the bar.

**Themes.** Light by default, dark on request, switchable under **You →
Settings** and saved with the rest of your progress. Colours are chosen per
theme rather than shared: a blue legible on near-black washes out on white.

## The dictionary

`scripts/build-dict.py` downloads two freely-licensed sources and compiles them
into one file, `assets/wordtee.dict`:

| Source | Used for | Licence |
| --- | --- | --- |
| [minhqnd/dictionary](https://github.com/minhqnd/dictionary) v2.0.0 (179 MB SQLite, from Wiktionary and TVTD) | headwords, senses, IPA, examples, synonyms | CC BY-SA |
| [hermitdave/FrequencyWords](https://github.com/hermitdave/FrequencyWords) `en_50k` (OpenSubtitles) | frequency ranking | CC BY-SA |

The app also bundles [Noto Sans](https://fonts.google.com/noto) (OFL 1.1, in
`assets/fonts/`). egui's own font covers only 89% of what this app draws: it has
neither the Vietnamese tone marks of Latin Extended Additional nor the IPA, so
`ừ`, `ế`, `ə` and `ɪ` all rendered as empty boxes.

**Everything the app draws comes from that one font**, and two tests hold the
line — one over every character in the dictionary, one over the app's own
labels, read straight out of the source. The second exists because the first
was not enough: the back arrow `←`, the close `✕` and the tab bar's emoji are
in no bundled font either, and the emoji that did resolve came from a fallback
in a different typeface. They are all words or in-font characters now.

```sh
python3 scripts/build-dict.py        # ~19 MB out, a few minutes the first time
```

The result is **19 MB**: 119.296 headwords, 190.051 senses, 25.000 ranked
learning items, 21.238 inflection→lemma mappings and a Vietnamese reverse
index. It is checked in, so `cargo build` and `docker compose build` work
without running the script.

`src/dict.rs` reads it zero-copy straight out of the `include_bytes!`
`&'static [u8]` — `Dict::load()` costs no measurable time and parses nothing at
startup. That is why there is no SQLite, no C toolchain and no `getrandom`
wiring: the identical code path runs on all three platforms.

Two things the build script does that are worth knowing:

- **Inflections come from the data.** The source has no inflection table, but
  ~22.000 entries are glossed "số nhiều của …", "quá khứ và phân từ quá khứ của
  …" and so on. Parsing those gives spec 1.1's `word_inflections` for free, and
  marks those entries as pointers rather than senses so they are never taught.
  Regular forms the source does not gloss are handled by rule at lookup time.
- **Gaps are filled from the other half of the source.** Wiktionary is missing
  `why`, `city` and `easy` outright, and has `new`, `eat` and `music` with no
  definitions at all. The Vietnamese side of the same database carries English
  translations, and inverting those recovers 1.129 headwords — `why` → "vì sao",
  `city` → "thành phố". Only used where there is nothing else.

## What this build does not do

The spec describes a product with an editorial pipeline behind it (spec 4.1) and
a roadmap that puts much of it in V2 and V3. This build is the MVP column, and
these are the honest gaps:

| Spec | Status |
| --- | --- |
| 1.3 Simple English definitions + VI toggle | The source has **no** English definitions for English headwords. Vietnamese only, so there is nothing to toggle. |
| 1.3 "Cách dùng": collocations, grammar patterns, register, UK/US, Vietnamese-learner errors | Not shipped — all authored content (spec 4.1). What the data supports *is* shown: word family, synonyms, antonyms, and phrases built on the word. |
| 1.3 Media layers 2–4 (images, video snippets, quotes, news) | Not implemented. Licence-gated and V2/V3 in the spec's own roadmap. |
| 1.2 Phrasal verbs as their own entries | The source has 8.154 phrase entries (idioms, noun phrases) but is missing the common phrasal verbs — no `give up`, `look up`, `run out of`. Nothing to show. |
| 1.4 On-device TTS | Web only, via the browser's speech synthesis. Desktop and Android would each need a platform binding; the IPA and examples are shown either way. |
| 1.4 Audio packs, LRU cache, sync | No audio files ship, and there is no server — progress is local, as asked. |
| 1.2 / 2.1 Sense ranking | The spec samples corpus sentences and has a model label each with a sense. No tagged corpus ships, so a headword's Zipf score is split across its senses as 1/(i+1) in dictionary order. Order *within* a headword is right; the split between headwords is an estimate. |
| 2.1 Frequency formula | Spoken only (OpenSubtitles). COCA and BNC are commercial, so the 0,6/0,4 blend and Juilland's *D* dispersion cannot be computed from the aggregated counts available. |
| 1.1 Double Metaphone | Folded into the edit-distance tier, which already covers both of the spec's own examples (`teh`, `fonetic`) at two edits. |
| 2.3 Relevance: `Goal`, `Topic` | Weighted 0 — they need the goal packs of spec 5.1 and per-item topic labels, neither of which exists in free data. `Lookup` and `Family` are computed and do work. |
| 3.2 Re-fitting FSRS per user after ~1.000 reviews | Ships the FSRS-5 defaults; no re-fit. |
| 5.1 / 5.3 / 5.4 / 5.6 Goal packs, lookup outside the app, text analyzer, pronunciation | Not implemented — V2 in the spec's roadmap. |

One number worth stating plainly, measured end to end by the same bench — a
word counts as answered if the pipeline returns it, whether as its own entry or
by resolving it to a lemma:

| Frequency band | Answered | Own entry | Via a lemma |
| --- | --- | --- | --- |
| top 1.000 | **95,9%** | 922 | 37 |
| top 3.000 | 93,3% | 2.623 | 175 |
| top 10.000 | 85,4% | 7.796 | 742 |

The remaining misses are the source's, and a good share of them are not words:
subtitle-tokeniser artefacts (`didn`, `'re`), fillers (`uh`, `hmm`) and proper
nouns.

## Docker

One build produces the APK *and* the browser version, and one image serves a
page that offers both — a live demo you can tap, and a download link for the APK
it just built:

```
┌───────────────────────────────────────────────────┐
│  ⬤ WordTee                                        │
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
docker compose build                            # → image wordtee-web
docker run --rm -p 8080:80 wordtee-web          # → http://localhost:8080
```

| Command | Effect |
| --- | --- |
| `docker compose build` | Build the APK, the wasm bundle and the image |
| `APK_ABI=all docker compose build` | …with an APK for all three ABIs |
| `docker run --rm -p 8080:80 wordtee-web` | Serve it on `localhost:8080` |
| `docker run --rm -v "$PWD/out:/out" wordtee-web cp /usr/share/nginx/html/download/wordtee.apk /out/` | Copy the APK onto the host |

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
  hold `wordtee.apk` and `build-info.json`; the healthcheck fails too in
  that case, so Coolify will mark the service unhealthy.
- **Build times out** — raise the deployment timeout in Coolify, or set
  `APK_ABI=arm64` (the default) rather than `all`.

## Layout

| Path | What it is |
| --- | --- |
| `assets/wordtee.dict` | The dictionary, 19 MB, `include_bytes!`d into the binary |
| `assets/fonts/` | Noto Sans, the one font that covers both Vietnamese and IPA |
| `scripts/build-dict.py` | Builds that file from the two upstream sources |
| `src/dict.rs` | Zero-copy reader for the pack |
| `src/search.rs` | The four-tier lookup pipeline (spec 1.1) |
| `src/srs.rs` | FSRS-5 (spec 3.2) |
| `src/progress.rs` | Per-item state, transitions, streak, daily limits (spec 0.2, 3.x) |
| `src/placement.rs` | The adaptive placement test (spec 2.2) |
| `src/study.rs` | Smart Feeding, the session queue, exercises (spec 2.3, 3.3, 3.6) |
| `src/quiz.rs` | Distractors and gap-fills, shared by both (spec 2.2, 3.3) |
| `src/rng.rs` | A small PRNG, so `getrandom` is not in the way on wasm |
| `src/app.rs` | The shell — tabs, toasts, saved progress — plus the UI tests |
| `src/ui/` | The four screens |
| `examples/bench.rs` | Checks lookup against the spec's latency targets |
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
uses Android's built-in `NativeActivity`, which loads `libwordtee.so` and calls
`android_main`.

## Desktop

```sh
cargo run            # debug
cargo run --release

cargo test                              # 92 tests, no window or GPU needed
cargo run --release --example bench     # lookup latency vs. the spec's budget
```

The tests run the real egui pipeline through `Context::run_ui`, so the screens
are exercised without a display. Most of them assert against the spec directly —
that `news` is not rewritten to `new`, that a session never pairs two synonyms,
that a pure guesser earns no frontier.

Progress is saved through eframe's storage: a file on desktop and Android, local
storage in the browser. Only items you have touched are stored; everything below
the placement frontier is assumed known by rule, which keeps a 25.000-item list
down to a few kilobytes.

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

The APK lands in `target/release/apk/wordtee.apk` (or `target/debug/apk/…`).
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
keytool -genkeypair -v -keystore ~/my-release.jks -alias wordtee \
        -keyalg RSA -keysize 2048 -validity 10000

export CARGO_APK_RELEASE_KEYSTORE=~/my-release.jks
export CARGO_APK_RELEASE_KEYSTORE_PASSWORD=...
./scripts/build-apk.sh --release
```

`build-apk.sh` leaves an externally-set `CARGO_APK_RELEASE_KEYSTORE` alone.

### Changing app identity

Everything lives in `[package.metadata.android]` in `Cargo.toml`:
`package` (`com.example.wordtee`), `label`, `apk_name`, `min_sdk_version` /
`target_sdk_version`, and the ABI list in `build_targets`.

### Logs from a device

```sh
adb logcat -s wordtee:V RustStdoutStderr:V
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
./scripts/apk-info.sh dist/download/wordtee.apk > dist/download/build-info.json
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
- The 19 MB dictionary is embedded, which makes the wasm bundle 24 MB — 9.2 MB
  over the wire, since nginx gzips `application/wasm`. That is a heavy first
  load for a web demo, and the trade for it is that nothing is fetched
  afterwards. Desktop and Android pay it as file size rather than download, and
  the pages are faulted in from the executable on demand rather than read into
  memory at startup.
- Scheduling is day-granular and in UTC. FSRS intervals are days, so an hour
  either side of midnight changes nothing that matters.
