---
title: Rotators
description: Control GS-232, Winegard, rotctld, or virtual mounts for satellites, radiosondes, and passive sky scans.
---

Set **Rotator** to **Track** mode and wire a controller to its `control` input:

```patch
[channel] Radiosonde : 403 MHz
[source] GPS position : Antenna site
[tool] Aim : Az / El target
[output] Rotator : Track mode

Radiosonde --events--> Aim
GPS position --position--> Aim
Aim --control--> Rotator
```

## Several controllers

Wire as many as you like into `control`. The pass that climbs highest leads, since it has the shortest path and the strongest signal, and keeps the dish until it sets. A pass beats an aim or a sky scan. With none in view, the first to rise leads, so the dish waits at its rise. **Following** and **Pass** on the face show which one.

```patch
[tool] Satellite : NOAA 19
[tool] Satellite : METEOR-M2 3
[output] Rotator : Track mode

NOAA 19 --control--> Rotator
METEOR-M2 3 --control--> Rotator
```

A satellite runs its radios and recorders only while its rotator follows it, so a recorder never takes a pass the dish points away from. A **Sky scan** sweeps between passes and pauses while one is up.

## Hardware links and settings

- **Links:** **GS-232** (serial), **Winegard** Trav'ler (57600 baud ODU/MOT/DCM; auto-reads SM/LG motor steps, 445° az / 94° el limits, retries dropped moves), Hamlib **rotctld** (`127.0.0.1:4533`), or **Virtual** (6°/s az, 3°/s el).
- **Settings:** **Mode** (Track/Manual/Park/Stop), **Axes** (Az+El or Az), **Range** (360°/450°/540° overlap), **Heading** (compass offset of mount 0°), **Flip** (180° elevation zenith flip), **Step** (deadband to spare motors). Passes pre-plan overlap to avoid mid-pass unwinds.

## Track targets (Aim)

Wire any position decoder (Radiosonde, ADS-B, APRS) `events` + antenna `GPS position` → **Aim** → **Rotator**. Tracks newest target by default, or enter a serial/ID in **Follow**.

## Passive RF survey (Sky scan)

Wire `Device iq` → **Sky scan** → **Rotator**. Set band (**Offset**, **Width**), grid (**Az/El** bounds, **Step**), and **Dwell**. Sweeps alternating rows and plots a sky heat map + peak bearing (editing settings stops and clears).
