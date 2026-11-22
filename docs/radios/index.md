---
title: Supported radios
description: Compare every supported USB and network SDR visually, check discovery, and configure shared Device settings.
---

- **Max rate:** USB or Ethernet can cap lower (a Pluto over USB holds ~4 MS/s).
- **Bandwidth:** bounded by analog filter or rate (RSPduo Dual Tuner gets 1.536 MHz per tuner).
- **Bits:** SDRplay drops to 12 bits above 6 MS/s and 8 above 9.2 MS/s; Airspy HF+ delivers 16 bits after decimation.
- **DC spike:** **Yes**/**Small** enable **DC block** by default (except SoapySDR); **Corrected** means the SDRplay API removes it.
- AD9363 boards tune 325 MHz to 3.8 GHz instead of 70 MHz to 6 GHz.
- Packaged releases ship every driver except CR-8. Making a radio? Email [hi@jhaag.me](mailto:hi@jhaag.me).

## Connect a radio

USB radios appear under **Radios** on an empty Device node; see [USB radios](usb.md) for driver quirks and [Network and virtual sources](network.md) for remote servers, recordings, and the signal generator.

On Linux, install your radio's udev rules and join its group (usually `plugdev`), then replug. SDRmm never needs root. Missing radio? Click **Check hardware** or run:

```sh
sdrmm --doctor
```

## Device controls

Controls mean the same thing across radios. Non-obvious ones:

- **Converter (MHz):** up/downconverter LO offset so all dials show antenna RF (`9750` for a Ku-band LNB, `-125` for a Ham It Up).
- **Multi-lane radios:** gain rows label `iq1`, `iq2`, ... with an **All** row and `-`/`+` lane count buttons.
- **Footer:** live **Queue**, **Drops**, **Lost**, and **Clipping** counters.

### Calibration

**PPM** and **Converter** save per physical radio (USB serial or network address), applying across all workspaces. Give RTL-SDR dongles [unique serials](usb.md#serials) so calibration never swaps on replug.
