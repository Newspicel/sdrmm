---
title: Aviation and marine
description: Notes for Inmarsat, Iridium, UAT 978 MHz, and 406 MHz beacons.
---

ADS-B, ACARS, VDL2, HFDL, AIS, DSC, and NAVTEX need only a frequency and an `events` wire.

<figure>
  <img src="/docs/screens/ais-dark.png" alt="AIS ship positions plotted on the map" loading="lazy" />
  <figcaption>AIS vessel positions on Map.</figcaption>
</figure>

## Inmarsat and Iridium

- **Inmarsat Classic Aero:** pick **Channel** (P to aircraft, R/T from aircraft, or C signalling). No audio.
- **Iridium:** one 50 kHz channel, or set **Span** (1, 2.5, 5, or 10 MHz) to decode bursts across the middle 80% of a matching radio rate.
- **Inmarsat STD-C:** plots EGC sea areas (NAVAREA/METAREA, coastal, rectangles, circles) on **Map**.

## UAT

978 MHz US GA below 18,000 ft (needs device rate ≥ **2.083334 MS/s**). Aircraft and TIS-B merge into the ADS-B table and Map layer. **FIS-B** decodes text weather/NOTAMs (deduplicated every 10 min; no NEXRAD graphics or multi-uplink NOTAMs).

## 406 MHz beacons

First-generation COSPAS-SARSAT EPIRBs/ELTs/PLBs. Default 20 kHz channel centred on **406.034 MHz** covers 406.025–406.040 MHz. Logs 15-hex ID, country, protocol, self-test flag, and Map position (**Beacons** layer). Second-generation beacons are not decoded.
