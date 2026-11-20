---
title: Arrays
description: Synchronize coherent lanes for beamforming, stitching, spatial spectrum, correlation, polarimetry, DF, and passive radar.
---

```patch
[source] Devices : Coherent lanes
[source] GPS position : Site & heading
[tool] Array : Sync, geometry & cal
[tool] Beamformer : Steered beam
[channel] NFM : Demodulate
[output] Speaker

Devices --iq--> Array
GPS position --position--> Array
Array --array--> Beamformer
Beamformer --iq:beam--> NFM
NFM --audio--> Speaker
```

## Build an array

- **Multi-lane radio / array recording:** press **Make array** on the Device.
- **Separate Devices (e.g. [KrakenSDR](../radios/usb.md#krakensdr)):** wire each Device `iq` into Array lanes in antenna order and set **Tier** (**Shared clock** or **Shared LO**; hardware banks enforce their own tier). Once wired, tuning and gain move to the Array.

| Hardware | Tier | Noise source |
|---|---|---|
| KrakenSDR (5), KerberosSDR (4) | Shared clock (auto) | Built-in (Kraken disconnects antennas) |
| LimeSDR, USRP B210, bladeRF 2, AD936x, CR-8 | Shared LO | None |
| RSPduo Dual Tuner, other multi-lane Soapy | Shared clock | None |
| [Array recording](recording.md#play-an-array-recording) | As recorded | Recorded noise windows |

**Tuning:** **Together** (all processors except Stitch) or **Spread** (adjacent frequencies for Stitch). Press **Rec** to [record all lanes](recording.md#record-an-array) into one SigMF collection.

## Geometry and pointing

- **Geometry** (angles clockwise from forward): **UCA** (Radius, First, Winding), **ULA** (Spacing, Axis), or **Custom** (`x/y/z` m). **Max f** shows the spatial aliasing limit.
- **Forward:** **Fixed** true-north azimuth, or **Heading** from a wired GPS + **Mount** offset.

### Point it

Wire GPS `position` in either Forward mode whenever [Direction finder](direction-finding.md) needs geographic bearings.

## Calibrate

Select **Cal source**: **Noise** (built-in), **Pilot** (known carrier Offset/Width), **Emitter** (known true Bearing/Offset/Width), or **Off**. Runs on start, retune, and gain change (**Calibration** adds periodic **Check**, band **EQ**, and **Warm start**). Gain **Auto** requires a cal source.

## Processors

| Processor | Needs | Output / modes |
|---|---|---|
| [Direction finder](direction-finding.md) | Calibrated Array (Together) | Bearings (`events`, `bearing`) |
| **Beamformer** | Calibrated Array (Together) | `beam` IQ lane: `MRC`, `Beam`, `MVDR`, `Nulls`, `GSC`, `Cancel` (Main minus Refs), `CMA`; steers from **DF** or **Fixed** azimuth |
| **Stitch** | **Spread** Array | `wide` IQ lane at `rate × lanes`; **Blend** SNR or Equal |
| **Spatial spectrum** | Calibrated Array | Bearing vs frequency (**Map**) or time (**Trail**), Rel or True |
| **Correlator** | Array | Pairwise amplitude/phase, **Delay**, and coherence **Coh** |
| **Polarimeter** | Crossed **H**/**V** lanes | Stokes, **Pol**, **Tilt**, **Ellip**, **Hand**; `beam` **Matched** or **Cross** |
| [Passive radar](passive-radar.md) | 2+ lanes (Together) | Bistatic range-Doppler & aircraft tracks |

### Beamformer

Wire `Beamformer beam` to any channel, Scope, or Recorder just like a radio lane.
