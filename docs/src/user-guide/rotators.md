# Rotators

A **Rotator** points wherever its `control` wire says. Three nodes drive one:

| Node | Points at |
|---|---|
| **Satellite** | A satellite pass. See [Satellites](satellites.md) |
| **Aim** | Anything a decoder places on the map: a radiosonde, an aircraft, an APRS station |
| **Sky scan** | Each point of a grid in turn, to map signal level by direction |

The Rotator must be in **Track** mode to follow.

## Track a radiosonde

1. Wire a channel running the **Radiosonde** decoder to **Aim** `events`.
2. Wire a GPS or fixed position to **Aim** `position`. This is where the antenna stands.
3. Wire **Aim** `control` to the Rotator.

Aim follows the newest sonde it hears. Type a serial in **Follow** to stay on one. A position
without altitude is taken to be as high as the antenna. Any decoder with positions works the same
way, so ADS-B or APRS can be followed too.

## Passive RF survey

1. Wire a radio lane to **Sky scan** `iq`.
2. Wire **Sky scan** `control` to the Rotator.
3. Set the band (**Offset**, **Width**), the area (**Az from**, **Az span**, **El from**, **El to**)
   and **Step**, then press **Start**.

The scan visits each point, waits for the rotator to get there, and keeps the loudest level heard
during **Dwell**. Rows sweep back and forth so the rotator never swings round. Set **El from** and
**El to** the same for a single sweep along the horizon. The face shows the grid as a heat map
and reads out the peak bearing.

Changing the settings or wires stops a running scan. New settings clear the grid.
