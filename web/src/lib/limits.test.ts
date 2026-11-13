import { describe, expect, it } from "vitest";
import { MAX_EXTENT_M, MIN_ELEMENTS } from "../canvas/nodes/arrayGeometry";
import { calSourceOf } from "../canvas/nodes/arrayNode";
import {
  ARRAY_LIMITS,
  BAND_SEED_HZ,
  BEAMFORMER_LIMITS,
  CORRELATOR_LIMITS,
  DF_LIMITS,
  FUSION_LIMITS,
  HUNT_LIMITS,
  holds,
  LIGHT_SPEED_M_S,
  lowest,
  POLARIMETER_LIMITS,
  powersOfTwo,
  RADAR_LIMITS,
  SPATIAL_LIMITS,
  STITCH_LIMITS,
  scaled,
} from "./limits";

describe("generated limits", () => {
  it("steps past an open lower bound only", () => {
    expect(lowest({ min: 0, max: 1, above: true }, 0.001)).toBe(0.001);
    expect(lowest({ min: 2, max: 16 }, 1)).toBe(2);
    expect(scaled(RADAR_LIMITS.overlap, 100)).toEqual({ min: 0, max: 75 });
    expect(scaled(RADAR_LIMITS.os_rank, 100)).toEqual({ min: 50, max: 95 });
    expect(holds(RADAR_LIMITS.jerk, 0)).toBe(false);
    expect(holds(RADAR_LIMITS.jerk, RADAR_LIMITS.jerk.max)).toBe(true);
    expect(holds(RADAR_LIMITS.cpi_ms, RADAR_LIMITS.cpi_ms.max + 1)).toBe(false);
  });

  it("carries the wire constants", () => {
    expect(LIGHT_SPEED_M_S).toBe(299_792_458);
    expect(RADAR_LIMITS.surface_db).toEqual({ min: -3, max: 30 });
    expect(ARRAY_LIMITS.gain_db).toEqual({ min: -20, max: 80 });
    expect(MIN_ELEMENTS).toBe(ARRAY_LIMITS.lanes.min);
    expect(MAX_EXTENT_M).toBe(ARRAY_LIMITS.extent_m);
    expect(STITCH_LIMITS.report_ms).toBeGreaterThan(0);
  });

  it("seeds every variant inside its bounds", () => {
    const seed = RADAR_LIMITS.seed;
    expect(holds(RADAR_LIMITS.bandwidth_hz, seed.dvbt_bandwidth_hz)).toBe(true);
    expect(holds(RADAR_LIMITS.bandwidth_hz, seed.custom_bandwidth_hz)).toBe(true);
    expect(holds(RADAR_LIMITS.cma_taps, seed.cma_taps)).toBe(true);
    expect(holds(RADAR_LIMITS.cma_step, seed.cma_step)).toBe(true);
    expect(holds(RADAR_LIMITS.os_rank, seed.os_rank)).toBe(true);
    expect(calSourceOf("pilot", { kind: "noise" })).toMatchObject({
      bandwidth_hz: ARRAY_LIMITS.cal_bandwidth_seed_hz,
    });
    expect(holds(ARRAY_LIMITS.cal_bandwidth_hz, ARRAY_LIMITS.cal_bandwidth_seed_hz)).toBe(true);
    expect(holds(FUSION_LIMITS.half_life_s, FUSION_LIMITS.half_life_seed_s)).toBe(true);
  });

  it("lists the powers of two a bound holds", () => {
    expect(powersOfTwo({ min: 64, max: 512 })).toEqual([64, 128, 256, 512]);
    expect(powersOfTwo({ min: 100, max: 100 })).toEqual([]);
  });

  it("carries the processor and hunt ranges", () => {
    expect(DF_LIMITS.report_ms).toEqual({ min: 100, max: 10_000 });
    expect(scaled(DF_LIMITS.band.bandwidth_hz, 1e-3)).toEqual({ min: 0.1, max: 20_000 });
    expect(DF_LIMITS.station_len).toBe(64);
    expect(BEAMFORMER_LIMITS.nulls).toBe(3);
    expect(BEAMFORMER_LIMITS.band.bandwidth_hz.min).toBe(1_000);
    expect(POLARIMETER_LIMITS.band.bandwidth_hz.min).toBe(100);
    expect(SPATIAL_LIMITS.report_ms).toEqual({ min: 50, max: 2_000 });
    expect(CORRELATOR_LIMITS.integrate_s).toEqual({ min: 0.05, max: 600 });
    expect(HUNT_LIMITS.mount_offset_deg).toEqual({ min: -180, max: 180 });
    for (const band of [BEAMFORMER_LIMITS.band, SPATIAL_LIMITS.band, CORRELATOR_LIMITS.band]) {
      expect(holds(band.bandwidth_hz, BAND_SEED_HZ)).toBe(true);
    }
  });
});
