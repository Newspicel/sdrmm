---
title: Rotators
description: Control GS-232, Winegard, rotctld, or virtual mounts for satellites, radiosondes, and passive sky scans.
---

Set **Rotator** to **Track** mode and wire one controller to its `control` input:

```patch
[channel] Radiosonde : 403 MHz
[source] GPS position : Antenna site
[tool] Aim : Az / El target
[output] Rotator : Track mode

Radiosonde --events--> Aim
GPS position --position--> Aim
Aim --control--> Rotator
```

## Hardware links and settings

- **Links:** **GS-232** (serial), **Winegard** Trav'ler (57600 baud ODU/MOT/DCM; auto-reads SM/LG motor steps, 445° az / 94° el limits, retries dropped moves), Hamlib **rotctld** (`127.0.0.1:4533`), or **Virtual** (6°/s az, 3°/s el).
- **Settings:** **Mode** (Track/Manual/Park/Stop), **Axes** (Az+El or Az), **Range** (360°/450°/540° overlap), **Heading** (compass offset of mount 0°), **Flip** (180° elevation zenith flip), **Step** (deadband to spare motors). Passes pre-plan overlap to avoid mid-pass unwinds.

## Track targets (Aim)

Wire any position decoder (Radiosonde, ADS-B, APRS) `events` + antenna `GPS position` → **Aim** → **Rotator**. Tracks newest target by default, or enter a serial/ID in **Follow**.

## Passive RF survey (Sky scan)

Wire `Device iq` → **Sky scan** → **Rotator**. Set band (**Offset**, **Width**), grid (**Az/El** bounds, **Step**), and **Dwell**. Sweeps alternating rows and plots a sky heat map + peak bearing (editing settings stops and clears).
