---
title: Tools
description: Antenna calculator (MoM simulator), NanoVNA analyser, and codeplug radio programmer under Library → Tools.
---

> [!NOTE]
> NanoVNA and Radio programmer talk to USB devices plugged into the **server**, not the client browser.

## Antenna calculator

Designs and simulates dipoles, folded/inverted-V dipoles, EFHWs, ground planes, 5/8 verticals, J-poles, quad loops, and Yagis with a built-in method-of-moments solver:

- Cut list, current shading, **Trim** to resonance, SWR/impedance/Smith/pattern plots, **Matching** networks, wideband **Resonances** (`0.4×–3.2×`).
- **Save to NanoVNA** overlays the model on real sweeps; **Export** writes NEC or Touchstone `.s1p`. Cut 2–5% long and trim on site.

## NanoVNA

USB sweep analyser with **Live** continuous mode, SWR/return loss/impedance/phase/S21/group delay/cable **Distance** (TDR with velocity factor + **Low-pass grid**), 4 markers, band summaries, **Match to 50 Ω**, saved/overlaid antenna traces, `.s1p`/`.s2p`/CSV import-export, and guided SOLT **Calibrate**.

## Radio programmer

Supports **AnyTone AT-D890UV** and **Radtel RT-4D** (channels, zones, contacts, group/scan lists, radio IDs). **Write to radio** reads first and flashes only changed blocks; **Copy for that radio** converts between models; **Merge in** combines stored codeplugs.
