---
title: Your first receiver
description: Hear a local FM station with an RTL-SDR in four steps.
---

No hardware yet? Open **Library → Examples** and import **No radio** to run on the built-in signal generator.

```patch
[source] Device : RTL-SDR 2.4 MS/s
[output] Scope : Band view
[channel] WFM : Broadcast FM
[output] Speaker : Live audio
[output] Readout : RDS name & text

Device --iq--> Scope
Device --iq--> WFM
WFM --audio--> Speaker
WFM --events--> Readout
```

1. **Pick the radio.** Plug in your RTL-SDR, select it on the **Device** node, and set **2.4 MS/s**. Missing? Press **Check hardware** or see [RTL-SDR](../radios/usb.md#rtl-sdr).
2. **Add WFM.** Press **+ Add** → **WFM**, wire `Device iq → WFM iq` and `WFM audio → Speaker audio`, and dial a local FM station. The Device follows the channel automatically.
3. **Listen.** Press play on **Speaker** and raise Device gain without clipping. Silent? Drag WFM squelch far left and press **Resume audio** if shown. See [Troubleshooting](../reference/troubleshooting.md#spectrum-works-but-audio-is-silent).
4. **RDS & Rack.** Wire `WFM events → Readout` for station name and text. Press `p` on any node to pin it to **Rack**, and `v` to swap views.

<figure>
  <img src="/docs/screens/rds-dark.png" alt="Broadcast FM receiver with RDS station name, radio text, and alternate frequencies" loading="lazy" />
  <figcaption>WFM receiver with RDS station readout.</figcaption>
</figure>

Next: [Nodes and wires](concepts.md), **Library → Templates**, or [Modes](/modes).
