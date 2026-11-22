---
title: Transmit
description: Arm safely, send APRS, talk over NFM/AM/SSB with PTT, voice activity, or continuous, or relay baseband and lanes.
---

> [!CAUTION]
> Verify licence, band plan, power, and antenna load before pressing **Arm**. Armed state is never saved across reloads.

```patch
[source] Microphone : Send mode
[encoder] NFM encoder : Audio FX + limiter
[output] Transmitter : Arm, level & timeout
[source] Device : Use = Send or Both

Microphone --audio--> NFM encoder
NFM encoder --baseband--> Transmitter
Transmitter --transmit--> Device
```

## Radios that send

Set Device **Use** to **Send** or **Both** (**Receive** never keys TX). TX gain lives on the Device; transmit frequency must lie inside the Device's active window.

| Radio | Duplex | Notes |
|---|---|---|
| HackRF One | Half | RX pauses per burst; +14 dB Amp shared by RX/TX |
| ADALM-Pluto, AntSDR | Full | Per-lane TX attenuation |
| Aaronia SPECTRAN V6 | Half | RX pauses while sending |
| bladeRF, LimeSDR, USRP (Soapy) | Per board | Shared `TX` gain elements |
| Virtual Transceiver / Half-duplex | Full / half | Debug builds; other virtual radios hear it |

## Ways to feed a Transmitter

- **Message (e.g. APRS):** `Encoder baseband → Transmitter → Device transmit`. Set frequency (empty = Device centre), **Arm**, press **Send**.
- **Voice (NFM / AM / SSB):** `Microphone audio → Encoder → Transmitter → Device`. Set **Send** to **Push to talk** (hold key or `Space`), **Voice activity**, or **Continuous**. The mic only opens while it reaches an armed transmitter. See [Voice chat](voice-chat.md) for Discord and TeamSpeak. Wiring another channel's `audio` builds a squelch-keyed relay. Voice encoders include the [Audio FX](channels.md#audio) chain plus a hard modulation limiter.
- **Channel / lane relay:** wire channel `baseband` or `Device iq` directly into Transmitter (always set an offset frequency unless antennas are isolated).

## Level, timeout, and width

- **Level:** keep several dB below full scale.
- **Timeout:** caps single burst duration, disarming if exceeded.
- **Width:** 99% occupied bandwidth of the last burst; warns amber if wider than the mode's channel (lower level/audio). Rewiring an armed Transmitter takes it off the air.
