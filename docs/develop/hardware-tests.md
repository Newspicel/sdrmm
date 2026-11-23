---
title: Hardware tests
description: Ignored hardware suites for USB capture health, KrakenSDR arrays, real DF bearings, and passive radar benchmarks.
---

Ignored by default and never run in CI; run on idle radios.

## Capture health

End-to-end USB → DSP → publication loss check (`SDRMM_CAPTURE_DRIVER`: `hackrf`, `rtlsdr`, `both`; `SDRMM_CAPTURE_ALLOW_DROPS=1` measures overload):

```sh
SDRMM_CAPTURE_DRIVER=hackrf SDRMM_CAPTURE_RATE=8000000 SDRMM_CAPTURE_SECONDS=30 \
  cargo test -p sdrmm-engine --lib --no-default-features --features rtlsdr,hackrf \
  connected_radio_capture_health -- --ignored --nocapture
```

RTL-SDR hardware 8-bit byte counter check:

```sh
SDRMM_RTL_TEST_RATE=3200000 SDRMM_RTL_TEST_SECONDS=60 \
  cargo test -p sdrmm-device-rtlsdr --lib hardware_counter_stream_is_continuous -- --ignored --nocapture
```

## Tektronix RSA

```sh
cargo test -p sdrmm-device-tektronix -- --ignored --nocapture --test-threads=1
```

Streams every rate without loss, retunes and changes rate while streaming, checks a gain step moves the level by as much. The gain test needs FM broadcast on the input.

## KrakenSDR

```sh
cargo test -p sdrmm-device-rtlsdr kraken_ -- --ignored --nocapture --test-threads=1
cargo test -p sdrmm-engine --test integration -- --ignored --nocapture --test-threads=1 array_hardware::
SDRMM_BENCH_ASSERT=mac cargo test -p sdrmm-engine --release \
  --test integration radar_hardware::benchmark_radar -- --ignored --nocapture
SDRMM_RADAR_FM_HZ=<hz> cargo test -p sdrmm-engine --release \
  --test integration radar_hardware::kraken_fm_live -- --ignored --nocapture
```

Add `--features probe` to verify zero noise-source leakage; use `SDRMM_BENCH_ASSERT=pi5` for Raspberry Pi 5 budgets. `kraken_df_known_bearing` takes `SDRMM_DF_TX_HZ`, `SDRMM_DF_BEARINGS` (≥ 3 bearings, ≥ 10° off axis), `SDRMM_DF_RADIUS_M`, `SDRMM_DF_WINDING`, `SDRMM_DF_GAIN_DB`, `SDRMM_DF_BANDWIDTH_HZ`.

## Measured reference

KrakenSDR 1000–1004, 2.4 MS/s, Apple M4 Max. Thresholds derived from these runs: lane/pair coherence ≥ 0.5, purity ≥ 0.8, clipping ≤ 75%, +2° cal sigma, 6/12 dB eigenvalue source counting.

<details>
<summary>Full KrakenSDR clipping, phase drift, and passive radar benchmark tables</summary>

Noise-source solve across all 29 gain steps (lanes 2–5 vs lane 1):

| MHz | Min coh | Min pair | Min purity | Max clip | Phase drift from 0 dB |
|---:|---:|---:|---:|---:|---|
| 100 | 0.83 | 0.82 | 0.95 | 0.58 | -8.9° to +3.8° |
| 433.92 | 0.84 | 0.83 | 0.95 | 0.36 | -9.2° to +2.1° |
| 868 | 0.74 | 0.71 | 0.92 | 0.34 | -9.4° to +2.4° |
| 1090 | 0.91 | 0.91 | 0.97 | 0.28 | -9.3° to +2.5° |
| 1300 | 0.75 | 0.76 | 0.91 | 0.22 | -9.2° to +2.4° |
| 1700 | 0.68 | 0.67 | 0.89 | 0.06 | -9.8° to +1.9° |

Passive radar median over 10 CPIs (M4 Max):

| Stage | FM | Target | DAB | Target |
|---|---:|---:|---:|---:|
| Front (core share) | 0.05 | 0.10 | 0.11 | 0.15 |
| CPI, 1 thread | 6.3 ms | 25 ms | 62 ms | 120 ms |
| CPI, 3 threads | 5.2 ms | | 40 ms | 50 ms |
| CPI, Auto | 5.3 ms (CPU) | | 23 ms (GPU) | 30 ms |

Lock at 433.92 MHz / 30 dB solved in 2.3 s (drift within 0.007 samples & 0.9° over 18 checks); retune re-solved in 0.69–0.71 s; 0 leaked noise blocks; 0 false sources across 357 noise-only DF reports.

</details>
