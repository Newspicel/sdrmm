# Feature roadmap

Remove an item once it ships.

## 1. Transmit

Already there: TX streams on HackRF, AD936x/Pluto and Soapy, a reserved `tx` input on transmit-capable
Device nodes that takes no wire yet, and modulators for NFM, AM, SSB, APRS, DMR, dPMR, D-Star,
M17, NXDN, P25 and YSF. Only the Signal generator uses the modulators.

### Foundation
- Transmit gate: the only node that may feed a Device's `tx` input
- Engine TX path: modulator to `TxStream`, settings via command queue
- Safety: band limits, TX timeout, a visible on-air state
- PTT: button, hotkey, VOX
- Microphone input into the voice modulators
- TX level metering and spectral mask check

### Sources
- Modulators for the remaining modes, over the frame/bit codec each protocol already owns
- IQ recording playback to air
- Signal generator and arbitrary waveforms to air

### Links
- Satellite uplink on the Doppler-corrected frequency the Satellite node already shows
- Bench loopback: TX into your own RX to validate decoders. The graph's no-cycle proof stops
  being sufficient here
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

## 3. Decoders

- GSM downlink analysis, OsmocomBB-style monitoring

## 4. Recording & measurement
- Recording scheduler and unattended satellite-pass automation
- Radio astronomy, star tracker, sky map

## 5. Automation & API
- WASM plugin SDK
