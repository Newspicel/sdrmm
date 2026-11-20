---
title: Broadcast and video
description: Notes for DAB/DAB+, DRM, HD Radio, DVB-T/T2, DATV, ATV, and SSTV.
---

<div class="shots">
  <figure>
    <img src="/docs/screens/sstv-dark.png" alt="Robot 36 SSTV received picture" loading="lazy" />
    <figcaption><strong>SSTV:</strong> Robot 36 with slant correction.</figcaption>
  </figure>
  <figure>
    <img src="/docs/screens/atv-dark.png" alt="625-line analog television picture" loading="lazy" />
    <figcaption><strong>ATV:</strong> 625-line PAL with Transform PAL chroma split.</figcaption>
  </figure>
</div>

## DAB and DAB+

2.048 MS/s, auto-corrects up to 40 kHz offset. **Readout** shows dynamic label and slideshow; **Decoder log** offers MOT file downloads; IP services feed [IP data](../network-export.md#ip-data).

## DRM

**Mode** picks DRM30 (A–D), DRM+ (E), or Auto; **BW** sets 4.5–20 kHz on DRM30 (tune reference frequency). Plays AAC, HE-AAC, HE-AAC v2, and xHE-AAC.

## HD Radio

Hybrid FM primary sidebands at 558 kS/s (HD1–HD8). Decodes SIS metadata, dynamic labels, and HDC/HE-AAC audio (parametric stereo plays mono). Extended sidebands and AM hybrid not yet decoded.

## DVB-T/T2 and DATV

Wire `audio` → **Speaker** and `video` → **Video**.

- **DVB-T/T2:** set **Standard** and **BW** (`1.7 MHz` fits 2.048 MS/s); optionally pick **Low priority** or **PLP**.
- **DVB-S/S2:** set **Rate** (100 kBd–4 MBd; radio needs `rate × (1 + roll-off)`). DVB-S auto-detects FEC (0.35 roll-off); DVB-S2 auto-detects MODCOD/VL-SNR (set **Roll-off** and optional multistream **Stream**). GSE feeds [IP data](../network-export.md#ip-data).
- **Superframes:** DVB-S2X Annex E formats 0–7 (set **Ref/Data code** or enable **Code search**).

## ATV

625, 525, or 405 lines; AM (tune vision carrier; VSB Nyquist slope) or FM. **Colour:** PAL (Transform PAL; **Split 3D** sharpens stills at +3 frames / ~2× CPU), NTSC (adaptive line comb), or SECAM (625 lines).

## SSTV

Tune USB dial frequency. **Mode** defaults to **Follow VIS**; keep **Slant** on; **Partial** saves faded passes. Supports Robot 36/72, Martin M1/M2, Scottie S1/S2/DX, PD50/90/120/180, Wraase SC2-180. Wire `video` → **Video** to save PNGs.
