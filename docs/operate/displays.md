---
title: Displays and logs
description: Scope, Readout, Decoder log, Map, Video, Export, and Event filter.
---

```patch
[source] Device : Radio
[channel] ADS-B : 1090 MHz
[output] Scope : Spectrum & waterfall
[output] Readout : Live table
[tool] Event filter : Keep / Drop
[output] Map : Tracks & globe
[output] Decoder log : History & clips

Device --iq--> Scope
Device --iq--> ADS-B
ADS-B --events--> Readout
ADS-B --events--> Event filter
Event filter --events--> Map
Event filter --events--> Decoder log
```

<div class="shots">
  <figure>
    <img src="/docs/screens/spectrum-dark.png" alt="Scope spectrum and waterfall with tuned channel marker" loading="lazy" />
    <figcaption><strong>Scope:</strong> drag markers to tune; toggle Band plan ruler.</figcaption>
  </figure>
  <figure>
    <img src="/docs/screens/adsb-dark.png" alt="Map with ADS-B aircraft tracks and decoder log" loading="lazy" />
    <figcaption><strong>Map &amp; Decoder log:</strong> live tracks, tables, and history.</figcaption>
  </figure>
</div>

## Output nodes

| Node | Wire from | Notes |
|---|---|---|
| **Scope** | `iq` | WebGL2 waterfall, draggable channel markers, regional band-plan ruler, IF passband/notch overlay |
| **Readout** | `events` | Live structured state (RDS text, aircraft/vessel/LoRa tables, DAB slideshows) |
| **Decoder log** | `events` | Persistent history grouped by **List**, **Frequency**, or **Station**; plays 8 kHz call clips (kept 24h) and offers safe file downloads (DAB MOT, GOES EMWIN) |
| **Map** | `events`, `position` | Live tracks, sonde burst/landing forecasts, satellite footprints. Bottom-left gear picks style, 3D **Globe**, or custom XYZ / MapLibre URL (saved per browser) |
| **Video** | `video` | Live ATV/DVB video and still-picture gallery (SSTV, WEFAX, satellites, Wenet; PNGs kept 24h, max 512 / 256 MB; needs a `video` wire to save) |
| **Export** | `events` | On-demand CSV or JSON download of logged rows (for continuous sinks, see [Network export](network-export.md)) |

## Event filter

Place between a decoder and any `events` sink. Every active rule must match (**Keep** passes matches; **Drop** hides matches). Rules irrelevant to an event kind are ignored. Chain filters to combine logic (for example Keep POCSAG → Drop `TEST`). Applies to newly arriving events only.
