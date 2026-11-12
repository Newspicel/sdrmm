# Feature roadmap

Remove an item once it ships.

## 1. Transmit

Already there: a Transmitter node armed by hand, fed by a Modulator (NFM, AM, SSB voice from a
Microphone or a channel's audio, APRS messages, plugin transmitters), by a channel's baseband or
by a lane, on HackRF, AD936x/Pluto, Soapy and the virtual radios. Level, timeout, on-air state,
occupied width, a PTT key and VOX included.

### Foundation
- Per-service emission masks beyond the occupied width

### Sources
- Modulators for the remaining modes, over the frame/bit codec each protocol already owns
- Arbitrary waveforms to air

### Links
- Satellite uplink on the Doppler-corrected frequency the Satellite node already shows
- Beam-steering CW (TX MIMO)

### Security research (own DUT, contained link)
- ISM-band capture, decode, replay
- Fixed-code analysis and generation, including de Bruijn sequences
- Rolling-code capture and implementation analysis
- Interference and jam-susceptibility testing
- Flood, spam and malformed-broadcast testing
- Targeted protocol fuzzing

## 2. Codeplugs
- AnyTone D890UV: the general-settings block, GPS, both APRS flavours, roaming, encryption keys,
  DTMF/2-tone/5-tone, satellite and boot settings, and the per-channel long tail (custom CTCSS,
  talkaround, call confirm, ranging, scrambler, TX colour code) are not read
- AnyTone: transmit shift direction bits are inferred, not measured. Needs a radio read with a
  shifted channel
- Radtel RT-4D: settings blocks, keys and message templates are read but not modelled

## 3. Recording & measurement
- Recording scheduler and unattended satellite-pass automation
- Radio astronomy, star tracker, sky map

## 4. Automation & API
- WASM plugin SDK
