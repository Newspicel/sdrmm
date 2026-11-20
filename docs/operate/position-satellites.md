---
title: Position and satellites
description: Station GPS/heading sources, satellite pass prediction, automatic Doppler correction, and pass-gated hardware.
---

```patch
[source] GPS position : Fixed, serial, gpsd, or phone
[tool] Satellite : Orbit & Doppler
[source] Device : Opens 30 s before AOS
[channel] Meteor LRPT : Doppler-tracked
[output] Rotator : Track mode
[output] Map : Footprint & ground track

GPS position --position--> Satellite
Satellite --control--> Device
Satellite --control--> Meteor LRPT
Satellite --control--> Rotator
Satellite --events--> Map
```

## GPS position

One **GPS position** node can feed ADS-B, Array, Map, Passive radar (`tx`), Propagation map, Recorder, Satellite, Signal hunt, Signal survey, and Triangulation.

| Source | Details | Heading (true north) |
|---|---|---|
| **Receiver** | Serial/USB NMEA on server | `HDT` / `THS` (drops > 2 s old; magnetic ignored) |
| **Network** | `gpsd` (`127.0.0.1:2947`) | `ATT` reports (drops > 2 s old) |
| **Fixed** | Manual lat/lon | None |
| **Phone** | Paired [phone](phones.md) | Fused compass + gyro + GPS |

Serial and `gpsd` reconnect automatically; lost fixes stop feeding consumers.

## Satellites

1. Wire **GPS position** → **Satellite** `position`.
2. Search by name/NORAD ID (or paste TLEs) and pick a **Signal** (or dial manually).
3. Wire Satellite `control` → decoders (removes Doppler continuously; Device follows on Auto).

- **Pass-gated hardware:** wire Satellite `control` → Device `control` (opens radio 30 s before AOS, closes at LOS; manual **Stop** still wins) and armed Recorder `control`.
- **Antenna & Map:** wire `control` → [Rotator](rotators.md) (Track mode) and `events` → **Map** (try **Globe**).
- Orbits pull from CelesTrak and transmitters from SatNOGS DB (2 h cache; warns past 7 days old).
