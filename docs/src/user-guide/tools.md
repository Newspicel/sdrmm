# Tools

**Library → Tools** opens instruments and calculators that are not part of a receiver.

| Tool | Does |
|---|---|
| Antenna calculator | Dimensions for dipoles, folded and inverted-V dipoles, end-fed half-waves, ground planes, 5/8 verticals, J-poles, quad loops, and Yagis, with simulated SWR, gain, and radiation pattern |
| NanoVNA | Sweeps an antenna over USB and shows SWR, impedance, a Smith chart, and cable faults. Keeps sweeps per antenna to compare. |
| Radio programmer | Reads, merges, and writes codeplugs, and copies them between radios |

## Antenna calculator

Pick a design and a band or frequency. Every design is also simulated: the wires are solved
with the method of moments, over the ground you pick.

- **Design**: the cut list and a drawing. **Current** shades each wire by how much current it
  carries. Hover a part to find it in the drawing.
- **Trim** finds the element factor where the feedpoint is resonant.
- **Performance**: SWR, return loss and impedance over the band, a Smith chart, and the
  azimuth and elevation patterns. **Matching** compares baluns, ununs, a quarter-wave line,
  and L networks; pick one to see its SWR. The dashed trace is the bare feedpoint.
- **Resonances**: a sweep from 0.4 to 3.2 times the frequency. ▼ marks a series resonance,
  ▽ a parallel one. **Bands** shows which amateur bands the antenna also works on.
- **Save to NanoVNA** keeps the simulated curve in the NanoVNA antenna library, to overlay on
  a real sweep. **Export** writes a NEC model for 4nec2 or EZNEC, or a Touchstone `.s1p`.

The model ignores nearby metal, trees, and the feedline. Cut 2 to 5% long and trim on site.

## NanoVNA

Pick a band or type a range, then **Sweep**. **Live** sweeps again as soon as each sweep ends,
for trimming an antenna while you watch.

- **Chart**: SWR, return loss, impedance, phase, S21, group delay, and distance. Amateur bands
  of your region are shaded. Drag across the chart to zoom.
- **Markers**: the lowest SWR is marked on its own. Click the chart for up to four more; arrow
  keys move the selected one.
- **Bands**: best and worst SWR in each amateur band the sweep covers.
- **Match to 50 Ω**: L networks that match the selected marker.
- **Distance**: reflections along a cable. Set the velocity factor of the cable. **Low-pass
  grid** moves the start so the impedance step can be shown too.
- **Antennas**: name an antenna, then **Save** sweeps to it. Show a saved sweep as the main
  trace, or overlay up to four. **Open .s1p or .s2p** loads files from other tools.
- **Export**: Touchstone `.s1p`, `.s2p`, or a CSV table.

**Calibrate** walks through open, short, load, and the optional isolation and thru steps over
the current range, and saves the result in a slot on the device. **Device** sets IF bandwidth
and drive.

## Radio programmer

Supported radios: AnyTone AT-D890UV and Radtel RT-4D.

It handles what both radios share: channels, zones, contacts, group lists, scan lists, and radio
IDs. **Write to radio** reads the radio first and writes back only the blocks that differ, so the
rest of the codeplug stays as it was.

**Copy for that radio** fits a codeplug to the model picked under **Copy to** and stores it as a
new one. **Merge in** takes entries from another stored codeplug.

The NanoVNA and the radio programmer need the device plugged into the server, not the client.
