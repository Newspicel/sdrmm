import { describe, expect, it } from "vitest";
import { MAX_EXTENT_M, MIN_ELEMENTS } from "../canvas/nodes/arrayGeometry";
import { calSourceOf } from "../canvas/nodes/arrayNode";
import {
  ARRAY_LIMITS,
  FUSION_LIMITS,
  HUNT_LIMITS,
  holds,
  LIGHT_SPEED_M_S,
  lowest,
  powersOfTwo,
  RANGE_DOPPLER_DB,
  scaled,
} from "./limits";

describe("generated limits", () => {
  it("steps past an open lower bound only", () => {
    expect(lowest({ min: 0, max: 1, above: true }, 0.001)).toBe(0.001);
    expect(lowest({ min: 2, max: 16 }, 1)).toBe(2);
    expect(scaled({ min: 0, max: 0.75 }, 100)).toEqual({ min: 0, max: 75 });
    expect(holds({ min: 0, max: 1, above: true }, 0)).toBe(false);
    expect(holds({ min: 0, max: 1, above: true }, 1)).toBe(true);
    expect(holds({ min: 50, max: 2_000 }, 2_001)).toBe(false);
  });

  it("carries the wire constants", () => {
    expect(LIGHT_SPEED_M_S).toBe(299_792_458);
    expect(RANGE_DOPPLER_DB).toEqual({ min: -3, max: 30 });
    expect(ARRAY_LIMITS.gain_db).toEqual({ min: -20, max: 80 });
    expect(MIN_ELEMENTS).toBe(ARRAY_LIMITS.lanes.min);
    expect(MAX_EXTENT_M).toBe(ARRAY_LIMITS.extent_m);
  });

  it("seeds every variant inside its bounds", () => {
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

  it("carries the hunt ranges", () => {
    expect(HUNT_LIMITS.mount_offset_deg).toEqual({ min: -180, max: 180 });
  });
});
