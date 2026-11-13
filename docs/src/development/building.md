# Build and test

CI and local development run the same `cargo xtask` commands.

## Prerequisites

| Tool | Version |
|---|---|
| Rust | 1.99.1, pinned in `rust-toolchain.toml`; rustup installs it |
| Node | 26 |
| pnpm | 12, exact version in `web/package.json` |
| FFmpeg | 9, built by `scripts/build-media.py` |
| Native | C/C++ compiler, Clang/libclang, CMake, GNU Make, NASM, Python 3.12+ |

```sh
sudo apt-get install -y build-essential cmake clang libclang-dev python3 nasm   # Debian, Ubuntu
brew install cmake python nasm                                                  # macOS
```

Fuzzing and `cargo xtask sanitize` also need a nightly toolchain. SoapySDR loads at runtime, so no
development package is needed.

## Build and run

```sh
git clone https://github.com/Newspicel/sdrmm.git
cd sdrmm
python3 scripts/build-media.py
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
cargo run -p sdrmm
```

Open <http://localhost:8080>.

The media script builds the few FFmpeg 9.0.2 codecs SDRmm needs, plus fdk-aac 2.0.3 for xHE-AAC,
from checksummed source into `.media/<target>` as shared libraries. For the host target it also
writes `.media/cargo.toml`, which `.cargo/config.toml` includes, so every Cargo command uses that
FFmpeg (`FFMPEG_DIR`, plus `LD_LIBRARY_PATH` on Linux or `DYLD_LIBRARY_PATH` on macOS). On
Windows, also add `$FFMPEG_DIR/bin` to `PATH`. A system FFmpeg older than 9 does not compile. For cross builds,
pass `--target <triple>` to the script. Nix uses its own FFmpeg with fdk-aac.

The server embeds `web/dist`, so build the frontend first. Without it, the server shows a
placeholder page.

For hot reload of the frontend and automatic backend restarts:

```sh
cargo xtask dev --watch
```

Open <http://localhost:5173>. Vite forwards API and WebSocket traffic to port 8080. Without
`--watch`, the backend runs once.

### Windows

Build from a Visual Studio developer shell with LLVM and MSYS2 Make installed; the CI Rust action
in `.github/actions/rust` shows the setup. Set `MEDIA_SHELL_BIN` to the folders holding `bash`,
`make`, and `clang-cl`, and `LIBCLANG_PATH` to libclang. The script adds those folders only for
the tools it runs, so MSYS2's `link.exe` never hides the MSVC linker.

ARM64 builds the codecs without assembly, because FFmpeg's ARM assembler tools do not ship with
the toolchain. xtask retries Cargo up to three times on Windows, since the
`ffmpeg-sys-the-third` build script sometimes crashes loading libclang
([issue 145](https://github.com/shssoichiro/ffmpeg-the-third/issues/145)).

## Feature flags

The server enables `soapy`, `sdrplay`, `aaronia`, `cr8`, `rtlsdr`, `hackrf`, `airspy`, `airspyhf`,
`funcube`, `espsdr`, `ad936x`, `net-client`, and `gpu-fft` by default. Packaged releases leave out
`cr8`. `dvsi` adds AMBE vocoder chips and is off by default.

```sh
cargo run -p sdrmm --no-default-features                        # no radio drivers
cargo run -p sdrmm --no-default-features --features net-client  # network radios only
```

Recording playback and the signal generator work in every build.

## Test without a radio

Add a **Signal generator** node, pick a signal, and wire it to a matching channel and a Speaker.

Debug builds also offer synthetic radios on the Device node.

| Radio | Key | Is |
|---|---|---|
| Kraken bench ×5 | `kraken5` | Five lanes on one clock with a noise source, like a KrakenSDR |
| Coherent Array ×4 | `array4` | Four lanes on one clock and LO, with a pilot tone |
| Dongle 1, Dongle 2 | `dongle1`, `dongle2` | One lane each, two elements of a line array; Dongle 1 holds the noise source |
| Test band | `band` | A band of test signals on one lane |
| Receiver ×4, Transceiver 2×2, Half-duplex 1×1 | `quad`, `transceiver`, `halfduplex` | Lane and transmit shapes |

The bench radios hear one FM emitter at 137° and its echo at 250°.
[Hardware tests](hardware-tests.md) cover real radios.

## Checks

| Command | Runs |
|---|---|
| `cargo xtask check` | Format, Clippy, web and site lint and type-check, crate rules, feature-set builds, generated-file drift |
| `cargo xtask test` | Generated fixtures, then Rust, web and site tests, with SoapySDR hidden |
| `cargo xtask smoke` | Playwright against a real `sdrmm` process |
| `cargo xtask perf` | DSP, channel and engine throughput and allocation gates |
| `cargo xtask audit` | RustSec advisories through cargo-deny |
| `cargo xtask desktop` | Clippy and tests on the desktop app, no installers |
| `cargo xtask sanitize` | Channel tests, with the vendored C, under AddressSanitizer and UBSan |
| `cargo xtask fuzz` | libFuzzer on decoders, parsers and importers |

```sh
cargo install --locked cargo-nextest cargo-deny cargo-fuzz
pnpm --dir web exec playwright install chromium
```

`test` needs cargo-nextest, `audit` cargo-deny, `fuzz` cargo-fuzz, and `sanitize` clang.
Automated tests never need real hardware.

`fuzz` runs every target in `fuzz/fuzz_targets/`, or the one `--target` names, for `--seconds`
(default 60) on `--jobs` workers. `--minimize` shrinks the corpus instead.

| Target | Feeds |
|---|---|
| `channel_chain` | IQ into every decoder |
| `channel_settings` | Random settings into every decoder |
| `dv_voice` | IQ into the digital voice decoders |
| `media_bitstream` | Audio codec bitstreams |
| `net_radio_proto` | rtl_tcp, SpyServer, KiwiSDR and SDRconnect messages |
| `iqlink_header` | iqlink packets |
| `recorder_import` | SigMF archives |
| `workspace_import` | Workspace exports |
| `cps_image` | Codeplug images |

CI runs `check`, `perf`, `smoke`, `test`, GPU parity, SIMD kernels, the ETSI table check and
cargo-deny's licenses, bans and sources on every push and pull request. `test` builds once per
architecture (`test archive`) and runs the archive in four slices (`test shard`). The nightly
workflow runs the rest: macOS tests, array realtime, desktop, mobile, iPhone, Android, container
image, Nix, advisories, sanitizers, four hours of fuzzing per target, and measurement suites.

## Performance baselines

Throughput gates compare a measurement with a committed baseline:

| Gate | Baseline |
|---|---|
| `ber::perf::tests::compare_perf_baseline` in `sdrmm-modem-test-support` | `crates/modem/baselines/perf_phase0.json`, rows tagged by host |
| `compare_*perf_baseline*` in `sdrmm-modem --test integration` | `crates/modem/baselines/*/*perf*.json`, rows tagged by host |
| `survey_every_channel_search_path` in `sdrmm-channels` | `crates/channels/baselines/channel_perf/<arch>-<os>.json` |

Every row carries the host it was measured on, `<arch>-<os>`. Gates compare only the rows of the
host they run on and fail when they find none, so a new runner architecture cannot quietly switch
them off.

Record on a quiet machine, in release, and commit the files:

```sh
export SDRMM_WRITE_BASELINES=1
cargo test -p sdrmm-modem-test-support --release -- --ignored --exact ber::perf::tests::write_perf_baseline
cargo test -p sdrmm-modem --release --test integration -- --ignored --test-threads=1 --skip compare_ perf_baseline
cargo test -p sdrmm-channels --release --test integration -- --ignored --exact channel_perf::write_perf_baseline
```

For a CI host, dispatch the `Perf baseline` workflow (or push to the `perf-baseline` branch),
download its artifacts, and commit them. Recording replaces the rows of the host that measured them
and keeps every other host's.

Each baseline also stores a reference kernel measurement, so a faster or slower machine of the same
host shifts the expectation instead of failing the gate. Run a gate on a machine that is otherwise
idle: the kernel is cache resident and barely notices a busy host, while a decoder reading tens of
megabytes does.

The modem gate allows 10% below the expectation. The channel survey allows 35%: it compares
every channel at once, and `ft8`, `ft4`, `wspr` and `stanag` are timed over one pass whose
deferred decode may or may not land inside the window.

## Generated files

Regenerate and commit these with the change that caused them:

| When you change | Run | Updates |
|---|---|---|
| REST routes or wire types | `cargo xtask codegen` | `openapi.json`, `web/src/generated/` |
| Dependencies | `cargo xtask licenses` | `THIRD_PARTY_NOTICES.md`, `crates/server/data/notices.json`, phone notices |
| Band-plan sources | `cargo xtask bandplan` | `crates/server/data/bandplan/` |
| `assets/icon.svg` | `cargo xtask icons` | Desktop, iPhone, web, site, and docs icons |
| Demo scenes in `web/e2e/scenes.ts` | `pnpm --dir web demo:record` | `site/public/demo/` |
| README screenshots | `cargo xtask screenshots` | `assets/screenshots/` |

`cargo xtask check` catches stale codegen and notices.

`cargo xtask fixtures` writes the synthesized decoder SigMF pairs into `fixtures/`. Git ignores
them, and `cargo xtask test` regenerates them.

## Other tasks

| Command | Does |
|---|---|
| `cargo xtask excerpt` | Trims a capture into a fixture; see `fixtures/README.md` |
| `cargo xtask net-capture` | Records a public KiwiSDR or SpyServer to SigMF |
| `cargo xtask replay` | Runs a capture through one channel; `--center` sets the RF centre of a raw file, `--images` saves pictures |
| `cargo xtask ber <entry>` | Bit error rate curves into `target/ber` |
| `cargo xtask ident-matrix` | Signal identifier against the fixtures |
| `cargo xtask compare <dsp\|decoders\|apps>` | Compares with other SDR software; `--ours` measures SDRmm alone, `decoders --signal <id>` one signal |

## Desktop app

The desktop app embeds Chromium through [cef-rs](https://github.com/tauri-apps/cef-rs). It is a
workspace member but not a default member. Its build downloads CEF into `target/cef` and needs
CMake and Ninja. On Linux, Chromium also needs its runtime libraries:

```sh
sudo apt-get install -y ninja-build libnss3 libatk-bridge2.0-0t64 libcups2t64 libgbm1 libxkbcommon0 libxcomposite1 libxdamage1 libxrandr2 libasound2t64
```

`cargo xtask desktop` lints and tests it, `cargo xtask desktop --run` starts it, and
[Releases](releases.md#desktop-bundles) builds installers.

## Before a commit

Format, lint, check, and test what you changed. For docs, run `mdbook build docs` and check links.
The full gates are `cargo xtask check` and `cargo xtask test`.
