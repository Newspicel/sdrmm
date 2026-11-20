---
title: Passive radar
description: Track aircraft bistatic range and Doppler speed using FM, DAB, or DVB-T broadcast illuminators.
---

Two shared-clock lanes are enough; phase calibration is only required for echo bearings (AoA).

```patch
[tool] Array : 2+ lanes, Together
[source] GPS (Fixed) : Illuminator site
[channel] ADS-B : Optional truth
[tool] Passive radar : Range-Doppler & tracks
[output] Map : Ellipses & positions

Array --array--> Passive radar
GPS (Fixed) --position:tx--> Passive radar
ADS-B --events:truth--> Passive radar
Passive radar --events--> Map
```

## Setup

1. Wire a **Together** [Array](arrays.md) → **Passive radar**. Point one antenna at the transmitter (**Ref**) and pick surveillance lanes (**Listen**).
2. Choose **Illum** (`FM` with CMA clean, `DAB` with remod clean, `DVB-T`, or `Custom` with **Bandwidth**) and set **Offset** (`tx freq - Array centre`).
3. For **Map**, wire station GPS to the Array, a **Fixed** GPS at the transmitter to `tx`, and optional ADS-B `events` to `truth` (tags matching call signs).

## Tuning and limits

- **Range / Speed / CPI / Overlap:** longer **CPI** detects weaker echoes but blurs fast targets.
- **Clutter:** `ECA-B`, `ECA-S`, `NLMS`, `Block NLMS`, or `Off` removes direct path and ground clutter.
- **Detect / Track:** CFAR (`CA`/`OS`/`GO`), **Pfa**, **Min SNR**, confirmation hits/looks, and **Coast**.
- **GPU:** **Auto** offloads CAF when faster. If `Overloaded`, shorten **Range** or **CPI**.
- Strong direct-path leakage into surveillance antennas caps range; lane rate below illuminator bandwidth reduces range resolution.
