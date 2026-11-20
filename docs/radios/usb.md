---
title: USB radios
description: Driver requirements, quirks, and platform setup for USB and Ethernet SDRs.
---

See [Supported radios](index.md) for specs, charts, and `sdrmm --doctor`.

## RTL-SDR

Built-in driver supports **R820T / R828D** tuners only (E4000, FC0012/13, FC2580 refused). Rates: 225–300 kHz or 900 kHz–3.2 MHz; BW: 290 kHz–8 MHz. **Direct sampling** (`off`/`i`/`q`) is hidden on Blog V4 / V4 Lite (built-in HF upconverter). Windows needs WinUSB via Zadig.

### Serials

Many dongles ship with `00000001`. SDRmm prompts once to write a unique serial to EEPROM (close the dongle, write, replug) so settings and calibration stick across USB ports. Clones without EEPROM cannot persist a serial.

## KrakenSDR

Five shared-clock tuners (`KrakenSDR <hub> lane 1`–`5`; KerberosSDR has four), max 2.56 MS/s, no vendor Pi image needed. Wire all lanes into one [Array](../operate/arrays.md) in antenna order (tier locks to **Shared clock**).

- **Lane 1** controls the noise source and all five bias tees, so it must stay open and wired.
- **Calibration:** Array triggers the noise source on retune/gain change and every minute (antennas disconnect and processors pause). Gain steps shift phase up to ~10°.
- **Errors:** `Noise clips, lower gain` (> 75% clipped); `Noise needs all lanes` (incomplete bank). Check **Coherent banks** in `sdrmm --doctor`.

Tested on hardware provided by [KrakenRF](https://www.krakenrf.com).

## HackRF

Built-in. LNA (8 dB steps), VGA (2 dB steps), +14 dB **Amp** (shared RX/TX), baseband **BW**, **Bias tee**. Give 20 MS/s its own USB 2 bus to avoid silent drops.

## Airspy

Built-in (omit `airspy`/`airspyhf` features to force SoapySDR).

- **R2 / Mini:** 24 MHz–1.8 GHz; LNA/Mixer/VGA in firmware steps; AGC on LNA, mixer, or both; bias tee.
- **HF+ / Discovery:** 1 kHz–31 MHz & 60–260 MHz; **Amp** (+6 dB), **Attenuator** (6 dB steps to -48 dB), low/high **AGC**, factory **PPM** init. Centres below 180 kHz (84 kHz narrow) clamp to that floor while still covering lower signals.

Tested on hardware provided by [Airspy](https://airspy.com).

## FUNcube Dongle Pro+

Built-in, fixed 192 kS/s. LNA/Mixer toggles, IF gain 0–59 dB (overload above ~25 dB with LNA+Mixer on), IF BW 200 kHz–8 MHz, RF filter, bias tee, PPM.

<details>
<summary>OS setup (Linux udev, Windows Zadig, macOS Microphone permission)</summary>

- **Linux:** detaches sound/HID drivers over raw USB. Add udev rule:
  `SUBSYSTEM=="usb", ATTRS{idVendor}=="04d8", ATTRS{idProduct}=="fb31", MODE="0660", GROUP="plugdev"`
- **Windows:** in Zadig, enable **List All Devices**, disable **Ignore Hubs or Composite Parents**, pick **FUNcube Dongle V2.0 (Composite Parent)**, install WinUSB.
- **macOS:** streams via Core Audio. Allow **Microphone** for SDRmm (or terminal) in System Settings, or samples read as silence.

</details>

Tested on hardware provided by [FUNcube Dongle](https://www.funcubedongle.com).

## ESP-SDR

Receives via ESP32 Wi-Fi radio flashed with [ESP-SDR](https://github.com/ESPARGOS/esp-sdr) ([browser flasher](https://espargos.net/espsdr/app/flash.html)). Reliable on 2.4 GHz (5 GHz on C5); 80/40/16 MS/s, 8 or 10 bits.

- **Serial:** captures bursts (4096 samples default, ~0.1 s gap at 921.6 kBd). Auto-probes 2 MBd, 1 MBd, 921.6 kBd (CP2102 needs `CONFIG_ESP_SDR_UART_BAUD=921600`). Linux requires `dialout` group.
- **Ethernet (ESP32-S31 `esp32s31-stream`):** gapless 4–40 MS/s at 8 bits (2150–2800 MHz); add via **Network → ESP-SDR** (Gigabit needed for 40 MS/s).

## AntSDR

Talks directly to Pluto firmware `iiod` (no `libiio`; UHD images unsupported, and `e310v2` does not run on E310). E310 RX1/RX2 share one synthesizer (phase coherent) and lock antenna/TX ports in firmware. Auto-discovers `ant.local` / `192.168.1.10`. USB works out of the box (Windows needs [PlutoSDR drivers](https://wiki.analog.com/university/tools/pluto/drivers/windows)).

<details>
<summary>Direct Ethernet setup and faster UDP streaming (sdrmm-iqlinkd)</summary>

**Direct Ethernet:** board defaults to `192.168.1.10`. Assign your Ethernet port static IPv4 `192.168.1.100/24` with **no gateway** (so internet stays on Wi-Fi). On Linux:
```sh
nmcli connection add type ethernet ifname <port> con-name antsdr ipv4.method manual ipv4.addresses 192.168.1.100/24
```
For router LANs outside `192.168.1.x`, connect over USB and edit `ipaddr_eth`/`netmask_eth` in `config.txt` (SSH login: `root` / `analog`).

**Faster UDP (`sdrmm-iqlinkd`, ~20 MS/s vs ~15 MS/s `iiod`):** auto-detected on UDP port `30432`:
```sh
rustup target add armv7-unknown-linux-musleabihf
cargo build --release --target armv7-unknown-linux-musleabihf -p sdrmm-iqlinkd
scp -O target/armv7-unknown-linux-musleabihf/release/sdrmm-iqlinkd root@192.168.1.10:/mnt/jffs2/
ssh root@192.168.1.10 'echo "/mnt/jffs2/sdrmm-iqlinkd > /tmp/iqlinkd.log 2>&1 &" > /mnt/jffs2/autorun.sh; reboot'
```

</details>

Tested on hardware provided by [MicroPhase](https://www.microphase.cn/).

## PlutoSDR and other AD936x boards

Direct `iiod` over USB or Ethernet (auto-discovers `pluto.local`, `192.168.2.1`, `ant.local`, `192.168.1.10`). Rates ~260 kS/s–61.44 MS/s (FPGA decimates below 2.08 MS/s); 2×2 boards are phase coherent.

- 8 fast-lock profiles cut repeat retunes from ~10 ms to ~2 ms; TX synth sleeps when idle.
- Extra chips expose per-lane AGC thresholds, quadrature/DC tracking, dual DDS TX **Tone**, chip state/calibration, external **LO**, and 1 Hz temperature/RSSI.
- Linux requires `libiio` udev rules (`sdrmm --doctor` checks them).

## SDRplay

Install [SDRplay API 3.15+](https://www.sdrplay.com/downloads/) and run `sdrplay_apiService` (no SoapySDR needed; no vendor code bundled). RF (LNA) and IF (0–39 dB) sliders both increase gain upward; AGC runs IF at 5/50/100 Hz.

- **RSPduo:** Tuner 1, Tuner 2, Dual Tuner (≤ 2 MS/s each), Master, and Slave appear as separate entries (busy modes hidden).
- Containers: see [Docker SDRplay](../server/deployment.md#sdrplay-receivers). Remote RSPs: see [SDRconnect](network.md#sdrconnect).

## Aaronia SPECTRAN V6

Supports **SPECTRAN V6 PLUS** and **SPECTRAN V6 ECO** via [RTSA-Suite PRO](https://aaronia.com/en/support/downloads) (Windows/Linux only; close RTSA-Suite before opening in SDRmm; run `setup_udev.sh` on Linux). Set `SDRMM_AARONIA_DIR` for custom paths.

- **V6 PLUS:** selectable `Rx1` / `Rx2` inputs, host-chosen receiver clocks up to 491.52 MS/s (80–245 MHz RTBW per model).
- **V6 ECO:** fixed 61.44 MS/s clock (40 MHz RTBW), single RX input.
- **GAIN** is inverted reference level (`20 dB` = `-20 dBm` full scale). Sample rate is `1.5 × span` (two-thirds clean); TX is half-duplex (0 dBm full scale before TX gain).

## Dragon Labs CR-8

8 phase-coherent lanes (`iq1`–`iq8`), 24 MHz–1.766 GHz, fixed 12.5 MS/s, internal/external clock. Excluded from packaged builds: compile with `--features cr8`, install the vendor SDK (override path with `SDRMM_DLCR_LIBRARY`), and verify via `sdrmm --doctor`.

## SoapySDR

Covers bladeRF (`SoapyBladeRF`), LimeSDR (`SoapyLMS7`), USRP (`SoapyUHD`), and remote servers (`SoapyRemote`). Loads at runtime (0.8 or 0.9); built-in drivers take precedence. Probed out-of-process for crash isolation (`SDRMM_SOAPY_PROBE=in-process` for debugging).

<details>
<summary>Install commands and custom library environment variables</summary>

| System | Core | Example module |
|---|---|---|
| Debian / Ubuntu / RPi OS | `sudo apt install libsoapysdr0.8` | `soapysdr-module-bladerf` |
| Fedora | `sudo dnf install SoapySDR` | `soapy-uhd` |
| Arch | `sudo pacman -S soapysdr` | `soapybladerf` |
| macOS | `brew install soapysdr` | `pothosware/pothos/soapybladerf` |
| Windows | [PothosSDR](https://github.com/pothosware/PothosSDR/wiki/Tutorial) | Included |
| NixOS | [`soapyPlugins`](../start/install.md#nix) | Included via override |

Custom paths: `SDRMM_SOAPY_LIBRARY`, `SOAPY_SDR_ROOT`, `SOAPY_SDR_PLUGIN_PATH`. Check detection with `SoapySDRUtil --find`.

</details>
