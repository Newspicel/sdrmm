# Satellites

The **Satellite** node tunes its decoders to a satellite's downlink and removes the Doppler
shift as it passes, so the decoder never has to chase the carrier.

## Build a satellite receiver

1. Add [GPS position](position.md). A fixed position is fine for a station that never moves.
2. Add **Satellite** and wire GPS `position` to its `position`.
3. Search by name or NORAD number, or paste element lines.
4. Pick a transmitter in **Signal**, or pick **Own frequency** and set the downlink.
5. Wire Satellite `control` to each decoder's `control`.

On auto tuning the radio follows the decoder, as it does for a scanner.

## On the map

Wire Satellite `events` to **Map**. It shows the point below the satellite, the footprint where it
is above the horizon, and the ground track half an orbit back and one ahead. The footprint is
brighter while the satellite is above your horizon. **Globe** in the map settings suits wide
footprints.

## Readouts

| Readout | Shows |
|---|---|
| Azimuth | Compass bearing |
| Elevation | Angle above the horizon; green while visible |
| Range | Distance to the satellite |
| Doppler | The shift being corrected, and how fast it changes |
| Send on | The Doppler-corrected uplink, if the transmitter has one |
| Next pass | Time until rise, or until set during a pass, with peak elevation |
| Elements | Age of the orbit data. Warns after 7 days; press the refresh button. |

Orbits come from CelesTrak and transmitters from SatNOGS DB. Both are cached for two hours.

What remains after correction is the radio's own oscillator error and a small orbit error.
Decoders that lock onto a carrier, such as DVB-S2, pull that in themselves.
