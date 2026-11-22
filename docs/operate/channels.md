---
title: Channels and audio
description: Tuning, auto vs manual Devices, squelch, IF filter, Audio FX, and virtual audio cables.
---

Wire `Device iq` into any mode channel from **+ Add** (see [Decoders](decoders.md) or [Modes](/modes)). Click a channel to inspect its baseband spectrum, constellation, and eye diagram in the side panel. Right-click → **Replace with...** swaps mode while keeping frequency and squelch (`m` / `M` cycles NFM, WFM, AM, SSB).

## Tuning

Dial digits accept scroll, arrow keys, or typed frequencies (MHz by default; add `Hz`, `kHz`, or `GHz`). You can also drag channel markers on **Scope** or use the [keyboard](../reference/keyboard.md).

### Auto and manual

- **Auto (default):** Device places its span to cover the most wired channels while avoiding its DC spike (`2/3` shows coverage).
- **Manual:** tuning the Device directly locks it in place; channels outside its span wait. Click the radar icon to return to Auto.
- **Several radios:** wire one channel to multiple Devices; auto-tuned radios split channels to maximize coverage. Scanners, hunts, recordings, and IQ exports pin their channel to one radio until stopped.

### Sample rate

SDRmm resamples Device IQ automatically. Matching native rate skips resampling (ADS-B `2.4 MS/s`, DAB/GNSS `2.048 MS/s`, ATV `16 MS/s`).

## Squelch

- **Off** (far left), **Manual** (slider), or **Auto** (**Margin** 2–40 dB above measured noise floor; freezes while open so long calls are not clipped). Analog channels emit call events only with squelch on.
- **NFM Tone:** **Detect** (display CTCSS/DCS), **CTCSS**, or **DCS** mute. **Scrambler** inverts voice (1.5–4.5 kHz or **Auto**); **Compander** applies 2:1 expansion.

## IF filter and blanker

- **Blanker:** removes short impulse spikes from IQ before filtering.
- **IF filter:** shapes IQ before demodulation so adjacent stations cannot capture FM or swamp a decoder. **Edges** trim each side independently; **Notches** pin to RF frequencies and stay on interferers when you retune (shaded/red on Scope).

## Audio

The **Audio** chain at the end of a decoder (or voice encoder) processes its `audio` output (**FX** lights in header). Stages run in order:

| Stage | Does |
|---|---|
| De-click | Removes clicks |
| Passband / Notches | Band-limits audio; cuts up to 4 tones |
| Peak | Boosts one tone up to +24 dB (useful for CW) |
| Auto notch | Tracks and cancels steady tones |
| Denoise | **Spectral** (lightweight, up to 20 dB) or **DPDFNet** (8 kHz neural speech model, download on demand: `2` light, `8` stronger; turn off for music/CW/data) |
| AGC | Slow / Med / Fast volume levelling (AM/SSB already level themselves) |

## Speaker output

Pick output device under **Out** (in browsers, press **Show device names** once). **Server** plays on the server's own sound card instead, for headless receivers; in Docker, pass `--device /dev/snd --group-add audio`. For external apps, route a Speaker to a virtual cable: **Linux** `pactl load-module module-null-sink sink_name=js8` (`js8.monitor`), **Windows** VB-Cable, **macOS** BlackHole.
