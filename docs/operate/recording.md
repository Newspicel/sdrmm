---
title: Recording and playback
description: Record IQ with pre-roll, channel baseband, audio, per-call WAVs, schedules, arrays, and SigMF playback.
---

| Node | Records | Wire from | Format |
|---|---|---|---|
| Recorder | Full Device IQ (+ pre-roll) | Device `iq` | SigMF (`.sigmf-data` + `.sigmf-meta`) |
| Baseband recorder | Filtered channel IQ | Channel `baseband` | SigMF |
| Audio recorder | Post-squelch/FX audio (**Skip silence** optional) | Channel `audio` | 48 kHz 16-bit WAV |
| Event output (Recordings) | One file per call | Channel `events` | 8 kHz 16-bit WAV |

**Record** persists with the workspace (continues with browser closed and across restarts). Clean shutdown finalizes open files.

```patch
[tool] Schedule : Time windows
[source] Device : Radio
[source] GPS position : Coordinates
[output] Recorder : SigMF + pre-roll

Schedule --control--> Device
Schedule --control--> Recorder
Device --iq--> Recorder
GPS position --position--> Recorder
```

## Record IQ, schedules, and pre-roll

- **IQ:** wire `Device iq` (and optional `GPS position`) to **Recorder**. Sample rate locks while recording; drops/restarts begin a new metadata segment instead of aborting.
- **Schedule / Satellite:** wire `Schedule` or [Satellite](position-satellites.md#satellites) `control` to Recorder `control` (and Device `control` to sleep hardware between windows), then arm **Record**.
- **Pre-roll:** enable **Pre-roll** (1–120 s, `seconds × rate × 8` bytes, max 1 GiB) to hold recent IQ in RAM and flush it first when **Record** starts.

## Record each call

Wire `events` and `audio` to **Event output → Recordings** (add **Event filter** ahead to skip short calls or filter talkgroups). Saves `YYYYMMDDTHHMMSSZ_<MODE>_<FREQ>.wav` per digital call or analog squelch opening (gaps < 1.5 s merge; > 10 min splits; analog requires squelch on; encrypted calls emit no audio). Works with [Spectrum monitor](finding-signals.md#monitor-a-band) too.

## Record an array

Press **Rec** on an [Array](arrays.md) to save all lanes into one `.sigmf-collection` (with geometry, tier, and noise windows). Press **Calibrate** while recording so playback calibrates cleanly.

## Play a recording

Open from **Library → Recordings → Open as source** (or **Device → Recording**). Plays IQ (and recorded GPS on `position`) with seek. **Upload SigMF** accepts `.sigmf` archives or `.sigmf-meta`/`.sigmf-data` pairs up to 16 GiB.

### Play an array recording

Opening a collection exposes `iq`–`iqN`. Press **Make array**, match the recorded geometry, wire GPS, and attach processors. Calibration replays from recorded noise windows.

## Storage and export

Edit name, tags, and notes in **Library → Recordings** (stored inside SigMF metadata). Download as `.sigmf` or stereo float `.wav` (not available for array collections). Default folder: `<data>/sdrmm/recordings` (`audio/` for WAVs; override with `--recordings-dir`; containers use `/data/recordings`).
