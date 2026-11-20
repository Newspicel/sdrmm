---
title: Utility and IoT
description: Notes for Pagers, DTMF, End-of-Train, ISM sensors, Wireless M-Bus, LoRa, Drone Remote ID, BLE, 2.4 GHz survey, Wi-Fi occupancy, RF test tools, and GNSS lab.
---

<figure>
  <img src="/docs/screens/pocsag-dark.png" alt="POCSAG pager decoding with webhook forwarding" loading="lazy" />
  <figcaption>POCSAG decoding wired to a webhook output.</figcaption>
</figure>

## Pager text

German POCSAG umlauts (`{ | } [ \ ] ~`) convert automatically inside lowercase words (`M}nchen` → `München`, leaving `[ALARM]` / ALL-CAPS intact). **Alpha only** drops numeric/tone pages.

- **Swissphone IDEA:** enter RIC message keys and PSW900 organisation code in POCSAG settings. Host clock (or recording capture time) must be within the transmission's local hour.

## DTMF

**Timing** Q.24 (≥ 40 ms) or **Fast** (20 ms). **Gap** sets silence ending a digit sequence.

## End-of-Train

Railroad telemetry: **457.9375 MHz** (rear unit), **452.9375 MHz** (locomotive), AU 477.7 MHz, NZ 450.2625 MHz. BCH-checked.

## ISM sensors

240 kHz channel decoding OOK/FSK sensors on 315, 345, 433.92, 868, and 915 MHz (2 s deduplication; **Unknown** logs repeating unrecognized pulses):

- **Supported:** Acurite (Tower, 5n1, 606TX, 609TXC), LaCrosse (TX141/TH, TX29IT, TX35DTH-IT), Fine Offset / Ambient Weather (WH2/5/x080/24/65B/31E/31B/40, F007TH), Oregon Scientific (THGR122N/228N/968, THN132N, WGR800, THGR810), Bresser (5in1, 6in1), Nexus, Prologue, Rubicson, Alecto V1, ThermoPro TP12, Schrader TPMS, Honeywell/2GIG, Kerui, PT2262.

## Wireless M-Bus

EN 13757-4 meters: modes T/C on **868.95 MHz**, mode S on **868.3 MHz** (own-modulator tested). Add meter ID + AES key under **Keys** for OMS mode 5 and Kamstrup ELL decryption.

## LoRa

Decodes **LoRaWAN**, **Meshtastic**, and **MeshCore** (locks up to BW/3 off centre). Pick a **Preset** or set **BW** (7.8–500 kHz), **SF** (single or **All** SF7–12), **IQ** (**Inverted** for LoRaWAN downlinks, **Both**), **Protocol**, and **Implicit** header fields.

- Meshtastic default preset keys and MeshCore **Public** are built in. Add custom **Keys** for Meshtastic (name + base64 PSK), MeshCore (name + secret; `#tag` needs none), LoRaWAN **ABP** (DevAddr/NwkSKey/AppSKey), or **OTAA** (AppKey).
- Positions plot on **Map**; E2E DMs flag as encrypted.

## Drone Remote ID

ASTM F3411, EN 4709-002, French beacons, and DJI DroneID (needs 2.4 GHz radio: HackRF/SDRplay/Pluto, not RTL-SDR). Templates: **Drone Remote ID** and **Drone Remote ID · Wi-Fi**.

| Link | Tune | Rate |
|---|---|---|
| Bluetooth | 2402, 2426, or 2480 MHz | 4 MS/s |
| BT band | 2.4 GHz (covers ~8 of 37 long-range data channels) | 20 MS/s |
| Wi-Fi (802.11b/g) | Channel centre (e.g. ch 6 = 2437 MHz, or 2412/2462/5745) | 20 MS/s |

## BLE advertisements

Lists devices, vendors, beacons (iBeacon, AltBeacon, Eddystone, Find My, Fast Pair, Exposure Notification), and scan/connect requests. **Channel** (4 MS/s on 2402/2426/2480 MHz) or **Band** (20 MS/s including long-range data). Template: **BLE devices**.

## 2.4 GHz survey and Wi-Fi occupancy

1 Hz energy monitors (no packet decoding; **Span** 20/40/80 MS/s; **Margin** over noise floor):

- **2.4 GHz survey:** classifies bursts into Wi-Fi, Bluetooth, 802.15.4, microwave oven, narrowband, wideband, and continuous.
- **Wi-Fi occupancy:** busy %, level, and noise floor per 20 MHz channel on 2.4, 5, and 6 GHz.

## Noise figure, PER tester and SID monitor

- **Noise figure:** Y-factor (400 kHz avg, LO excluded). Set **ENR**, fix gain, toggle calibrated noise source ≥ 0.5 s per state.
- **PER tester:** numbered 2-FSK frames (32b preamble, `1ACFFC1D`, 32b seq, PRBS9, CRC-16/CCITT); reports missed, rejected, BER, restarts. Signal generator transmits matching frames.
- **SID monitor:** tracks VLF 16–46 kHz (42 kHz span); flags > **Threshold** shift from 30-min baseline lasting 30 s.

## GNSS lab

Tune **1575.42 MHz** at **2.048 MS/s** with an active GPS antenna (enable bias tee). Tracks up to 12 of 32 PRNs; cold 3D fix takes 30–50 s with 4 satellites and plots on **Map** at 1 Hz with PDOP.
