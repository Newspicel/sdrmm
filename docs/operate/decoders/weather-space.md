---
title: Weather and satellites
description: Notes for Meteor LRPT, NOAA APT, HRPT, GOES HRIT, GK-2A LRIT, Wenet, WEFAX, Radiosondes, Horus Binary, and SAME alerts.
---

Wire `video` → **Video** to view and keep PNGs. Pair with [Satellite](../position-satellites.md#satellites) for Doppler tracking.

## Meteor LRPT and NOAA APT

- **Meteor LRPT (137.9 MHz):** QPSK 72k, OQPSK 72k (Meteor-M N2-4), or OQPSK 80k; composites ch 64/65 colour.
- **NOAA APT (137 MHz):** dual-channel AVHRR + telemetry wedges (**Partial** keeps faded passes; NOAA-15/18/19 decommissioned in 2025, so mainly for recordings).
- **HRPT (L-band):** Meteor-M (1700 MHz, ≥ 3 MS/s), NOAA (1698/1707 MHz, recordings), or MetOp-B/C (1701.3/1707 MHz, ≥ 4 MS/s). Needs tracking dish/helix + LNA. **Picture** picks colour, 11 µm IR, or Auto day/night.

## GOES HRIT and GK-2A LRIT

Needs ~1 m dish + L-band LNA aimed at the satellite.

- **GOES HRIT (1694.1 MHz, ≥ 2 MS/s):** ABI full-disk/mesoscale images; EMWIN/DCS files download from **Decoder log**.
- **GK-2A LRIT (1692.14 MHz):** add KMA DES keys under **Keys** by index to decrypt images.

## Wenet

Project Horus SSDV balloon photos near **443.5 MHz** (~300 kHz wide, tune midpoint of the two tones). Auto-detects 115k2 and v2 96k; GPS/text go to **Map** and **Decoder log**.

## WEFAX

Tune USB carrier **1.9 kHz below** published frequency. Start/stop tones pick IOC and end chart; fallback settings: **IOC** (576/288), **LPM** (60/90/120/240, usually 120), **Partial**.

## Radiosondes

RS41, DFM, M10, M20, iMet-4 (**Auto** runs all). RS41 temp/humidity appear ~1 min after lock once calibration subframes arrive. **Map** draws wind-derived burst/landing forecast using **Burst** and **Ground** altitudes.

## Horus Binary

100-baud 4FSK balloon telemetry (v1, v2, v3) with ±3 kHz acquisition. v1/v2 IDs map via the [Horus payload list](https://github.com/projecthorus/horusdemodlib/blob/master/payload_id_list.txt); v3 carries callsigns.

## SAME alerts

NOAA Weather Radio / EAS headers on 162.400–162.550 MHz (2-of-3 / byte-majority voting, 20 s timeout). Emits event, counties ([NWS SAME list](https://www.weather.gov/source/nwr/SameCode.txt)), duration, and `NNNN` end markers. No audio; pair with NFM (or use **NOAA Weather Radio** template).
