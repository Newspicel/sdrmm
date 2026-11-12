# Transmit

A Transmitter puts a signal on the air through a radio that can send. Nothing transmits until you
arm it, and arming is never saved: a workspace always opens with every transmitter off.

You are responsible for what you radiate. Check your licence, the band plan and your antenna
before you arm.

## Radios that send

| Radio | Duplex | Notes |
|---|---|---|
| HackRF One | Half | Receive pauses for each burst. The 14 dB amp is shared by receive and transmit |
| ADALM-Pluto, AntSDR | Full | TX attenuation per lane on the Device |
| bladeRF, LimeSDR, USRP (Soapy) | Per board | One `TX` gain per element the board has, set alike on every transmit channel |
| Transceiver, Half-duplex (virtual) | Full, half | Debug builds. What one virtual radio sends, the others hear |

The transmit gain is the `TX` gain on the Device; a Soapy board shows one per gain element, such
as `TX PAD`. The radio sends around the Device's centre, so
a transmit frequency has to fall inside the Device's window.

## Send a message

1. Add a **Modulator** and pick a mode that sends messages, such as APRS.
2. Add a **Transmitter**. Wire Modulator `baseband` to it, and its `transmit` to the Device's
   `transmit`.
3. Set the Transmitter's frequency. Empty means the Device's centre.
4. Press **Arm**.
5. Fill in the message on the Modulator and press **Send**.

The burst goes out once and the Transmitter returns to idle, still armed.

## Talk

1. Add a **Microphone**, a **Modulator** on NFM, AM or SSB, and a **Transmitter**.
2. Wire Microphone `audio` to the Modulator, then on to the Transmitter and the Device as above.
3. Arm, then hold the key on the Microphone while you speak. The [keyboard](keyboard.md) has a
   key for it too.

The browser asks for the microphone the first time. A channel's `audio` works in place of the
Microphone: the transmitter then sends whatever that channel hears, keyed by its squelch.

Turn on **VOX** to key by voice. The Microphone then listens all the time and sends while you are
louder than the threshold, and for the hang time after you stop.

## Send a channel

Wire a channel's `baseband` into a Transmitter to send that channel's filtered IQ again, on
another frequency or another radio. Without a frequency it goes out where it is heard, so set one
unless the two radios are far apart.

## Send a lane

Wire a Device's `iq` straight into a Transmitter to send that lane as it is: a recording, a signal
generator, or another radio. Without a frequency it goes out where it was received. A lane as wide
as the sending radio's window has to sit on that radio's centre.

## Level, timeout and state

| Setting | Meaning |
|---|---|
| Level | Digital drive below full scale. Leave a few dB of headroom |
| Timeout | The longest one transmission may last. Past it the Transmitter ends the burst and disarms |

The Transmitter shows when it is on the air, its frequency, the peak drive of the last burst and
how often the source could not keep up. **Width** is the band that held 99 % of the last
transmission's power. It turns to a warning when a modulator's signal is wider than its mode's
channel: lower the level or the audio. An error, such as a frequency outside the Device's window,
mutes it until the cause is gone.

Changing a Transmitter's wires while it is armed takes it off the air. Changing its source arms
it again on the new one.
