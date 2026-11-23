---
title: Build and test
description: Prerequisites, local dev loop, feature flags, xtask checks, fuzzing, baselines, and generated files.
---

## Build and run

Needs Rust `1.100.0` (`rust-toolchain.toml`), Node `26`, pnpm `12`, Python 3.12+, C/C++, Clang/libclang, CMake, GNU Make, and NASM (SoapySDR loads at runtime; no dev headers needed):

```sh
sudo apt-get install -y build-essential cmake clang libclang-dev python3 nasm   # Debian / Ubuntu
brew install cmake python nasm                                                  # macOS
```

```sh
git clone https://github.com/Newspicel/sdrmm.git
cd sdrmm
python3 scripts/build-media.py          # builds FFmpeg 9.0.2 + fdk-aac into .media/
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
cargo run -p sdrmm                      # http://localhost:8080
```

Hot reload (`http://localhost:5173` proxying `:8080`): `cargo xtask dev --watch`.

<details>
<summary>Windows build notes &amp; feature flags</summary>

- **Windows:** run from Visual Studio Developer Shell with LLVM + MSYS2 Make (`.github/actions/rust`); set `MEDIA_SHELL_BIN` (`bash`/`make`/`clang-cl`) and `LIBCLANG_PATH`. ARM64 builds codecs without asm.
- **Features:** default enables `soapy`, `sdrplay`, `aaronia`, `tektronix`, `cr8`, `rtlsdr`, `hackrf`, `airspy`, `airspyhf`, `funcube`, `espsdr`, `ad936x`, `net-client`, `gpu-fft`. Releases omit `cr8`; `dvsi` is opt-in.
  - `cargo run -p sdrmm --no-default-features` (no radio drivers; recordings & signal generator still work)
  - `cargo run -p sdrmm --no-default-features --features net-client` (network radios only)

</details>

## Test without a radio

Pick **Virtual** on an empty Device for the signal generator. Debug builds also expose synthetic radios: `kraken5` (5-lane shared-clock + noise), `array4` (4-lane shared-LO + pilot), `dongle1`/`dongle2` (2-lane ULA), `band`, `quad`, `transceiver`, and `halfduplex` (bench scenes hear FM at 137° + echo at 250°).

## Checks and tasks

| Command | Does |
|---|---|
| `cargo xtask check` | Format, Clippy, web/site lint + tsc, crate rules, feature checks, codegen drift |
| `cargo xtask test` | Synthetic fixtures + Rust/web/site tests (needs `cargo-nextest`) |
| `cargo xtask smoke` | Playwright smoke against live `sdrmm` |
| `cargo xtask perf` | DSP, channel, and engine throughput/allocation gates |
| `cargo xtask audit` | RustSec via `cargo-deny` |
| `cargo xtask desktop [--run]` | Lint/test (or launch) CEF desktop app (needs CMake + Ninja; Linux Chromium libs) |
| `cargo xtask sanitize` / `fuzz` | ASan+UBSan / libFuzzer (`channel_chain`, `channel_settings`, `dv_voice`, `media_bitstream`, `net_radio_proto`, `iqlink_header`, `recorder_import`, `workspace_import`, `cps_image`) |

## Generated files

| Changed | Run | Updates |
|---|---|---|
| Routes, wire types, catalog, decoders | `cargo xtask codegen` | `openapi.json`, `web/src/generated/`, `docs/reference/wires.md`, `docs/operate/decoders.md`, `site/src/data/modes.json` |
| Dependencies / bandplan / icon | `cargo xtask licenses` / `bandplan` / `icons` | Notices, bandplans, app/site icons |
| Demo scenes / screenshots | `pnpm --dir web demo:record` / `cargo xtask screenshots` | `site/public/demo/`, `assets/screenshots/`, `site/public/docs/screens/` |

<details>
<summary>Updating performance baselines &amp; extra developer tasks</summary>

Record per-host (`<arch>-<os>`) baselines in release on an idle machine (or dispatch the CI `Perf baseline` workflow):
```sh
export SDRMM_WRITE_BASELINES=1
cargo test -p sdrmm-modem-test-support --release -- --ignored --exact ber::perf::tests::write_perf_baseline
cargo test -p sdrmm-modem --release --test integration -- --ignored --test-threads=1 --skip compare_ perf_baseline
cargo test -p sdrmm-channels --release --test integration -- --ignored --exact channel_perf::write_perf_baseline
```

Extra tasks: `cargo xtask excerpt`, `net-capture`, `replay`, `ber <entry>`, `ident-matrix`, `compare <dsp|decoders|apps>`. See [Releases](releases.md#desktop-bundles-and-auto-updates) for packaging.

</details>

Before committing: format, lint, check, and test what you changed (`pnpm --dir site test` and `pnpm --dir site build` for docs).
