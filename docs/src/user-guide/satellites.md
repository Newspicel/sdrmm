# Satellites

The **Satellite** node tunes its decoders to a satellite's downlink and removes the Doppler
shift as it passes, so the decoder never has to chase the carrier.

## Build a satellite receiver

1. Add [GPS position](position.md). A fixed position is fine for a station that never moves.
2. Add **Satellite** and wire GPS `position` to its `position`.
3. Search by name or NORAD number, or paste element lines.
4. Pick a transmitter in **Signal**, or tune the downlink on the dial.
5. Wire Satellite `control` to each decoder's `control`.

On auto tuning the radio follows the decoder, as it does for a scanner.

## Point an antenna

1. Add **Rotator** and pick its link: a Yaesu **GS-232** controller on a serial port, or Hamlib
   **rotctld** (`127.0.0.1:4533` by default).
2. Wire Satellite `control` to the Rotator's `control`.

During a pass the rotator follows the satellite. Between passes it waits where the next one
rises.

| Setting | Does |
|---|---|
| Mode | **Track** the satellite, point by hand (**Manual**), **Park**, or **Stop** both axes |
| Axes | Azimuth and elevation, or azimuth only |
| Range | 360°, 450° or 540° of azimuth. Past 360° a pass crosses north without unwinding |
| Flip | Elevation reaches 180°. Passes over the top, or across the azimuth stop, are followed with the antenna flipped instead of swinging round |
| Step | Moves smaller than this wait, to spare the motors |

Each pass is planned before it rises, so the rotator never has to unwind halfway through.

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
