---
title: Direction finding
description: Compute live bearings from an Array, steer a Beamformer, and triangulate transmitters on the map.
---

```patch
[tool] Array : Calibrated UCA/ULA
[tool] Direction finder : Capon / MUSIC
[tool] Beamformer : Steered listening
[tool] Triangulation : Multi-bearing fix
[output] Map : Rays, heat & ellipse

Array --array--> Direction finder
Array --array--> Beamformer
Direction finder --bearing--> Beamformer
Direction finder --events--> Triangulation
Direction finder --events--> Map
Triangulation --events--> Map
```

## Direction finder

Wire a [calibrated Array](arrays.md#calibrate) (with GPS `position`) → **Direction finder**, set **Offset** and **Width**, and pick a **Method**:

- **Bartlett:** robust beam scan.
- **Capon:** sharper beam scan.
- **MUSIC:** resolves close emitters.
- **Root-MUSIC / ESPRIT:** grid-free (ULA or UCA only).

**Sources** auto-counts emitters or takes a fixed count; **More** adds multipath **Smooth** and **Yaw gate**. Wire `bearing` → [Beamformer](arrays.md#beamformer) `steer` to listen toward the signal.

<details>
<summary>Direction finder status chips</summary>

| Chip | Means |
|---|---|
| `Squelch` | Peak below Squelch threshold |
| `No heading` / `No position` | Missing GPS or heading on Array |
| `Aliased` / `Mode alias` | Element spacing > λ/2 or too high for UCA phase modes |
| `Mirror` | ULA front/back ambiguity (pick **Side**) |
| `Rotating` | Exceeded **Yaw gate** (blocks skipped) |
| `Singular` / `Table off` / `Stale` | Covariance failed, outside cal table, or 3 missed reports |

</details>

## Triangulate

Wire `events` from one or more Direction finders (or a [Signal hunt](finding-signals.md#sweep-for-a-bearing) sweep) to **Triangulation**, and wire both to **Map** for rays, heat layer, fix, and 1-sigma ellipse.

- **Align:** learns a fixed mount heading offset once a loop/crossing pins it down.
- **Fade:** ages old bearings (**Auto** picks Fixed/Moving, or **Set** half-life). **Min conf** / **Emitters** filter noise and cap targets.
- **Guidance:** wire vehicle GPS to `position` (`Drive across` / `Drive at it`; **Guide** Auto/Direct/Off, **Probe** cross distance). In a car with a [phone](phones.md), bearings pause while turning and down-weight sudden fades; cross from a second road angle to defeat multipath.
