---
title: Amateur and HF
description: Notes for FT8/FT4/WSPR/JT65/JT9, MSK144/Q65/FST4, JS8, APRS weather, BSS, and STANAG.
---

<figure>
  <img src="/docs/screens/ft8-dark.png" alt="FT8 decoded slots and messages" loading="lazy" />
  <figcaption>20 m FT8 slot decoding.</figcaption>
</figure>

## FT8, FT4 and WSPR

Slots time against server NTP clock (or playback time for recordings). Runs a second-pass search (like WSJT-X a7) for weak repeats of recently heard stations. Wire `events` to [Propagation map](../propagation.md) for activity and MUF.

**JT65 / JT9:** 1-minute slots; on shared dials raise **To** to `4000 Hz` to cover JT65 (< 2.5 kHz) and JT9 (> 2.5 kHz) together.

## MSK144, Q65 and FST4

- **MSK144:** 15 s meteor pings; **Audio** sets USB carrier (1500 Hz default) and **Tolerance** sets search width.
- **Q65:** match remote **Period** and **Submode** (A–E).
- **FST4 / FST4W:** match **Period** (FST4W ≥ 120 s). From 300 s, max span **From**–**To** is 1180 Hz (300 s), 580 Hz (900 s), 280 Hz (1800 s).

## JS8

Decodes Normal (15 s) and Slow (30 s) together; Fast and Turbo not yet decoded. Packed data frames show their raw 12 characters.

## APRS weather

Parses positioned and positionless weather packets into metric wind, gust, temp, rain, snow, humidity, pressure, and luminosity in **Readout**.

## BSS

1200 baud AFSK chat, positions, and call/location requests from Benshi handhelds (Btech UV-Pro, VGC VR-N76) on 2 m / 70 cm / GMRS. Can share a frequency with APRS; wire `events` to **Map**.

## STANAG modem identifier

Identifies HF USB data modems (STANAG 4285, 4529, 4539/110B, MIL-STD-188-110A serial with rate/interleave, 39-tone, 16-tone, 2G ALE). **Window** (2–12 s) trades latency for sensitivity. Template: **HF data modems**.
