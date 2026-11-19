import { describe, expect, it } from "vitest";
import { iqPowerDb } from "./basebandLevel";

describe("iqPowerDb", () => {
  it("reads a full-scale carrier as 0 dB", () => {
    const { meanDb, peakDb } = iqPowerDb(new Float32Array([1, 0, 0, 1, -1, 0, 0, -1]));
    expect(meanDb).toBeCloseTo(0);
    expect(peakDb).toBeCloseTo(0);
  });

  it("reads half amplitude as about -6 dB", () => {
    expect(iqPowerDb(new Float32Array([0.5, 0, 0.5, 0])).meanDb).toBeCloseTo(-6.02, 1);
  });

  it("reads silence as no level", () => {
    expect(iqPowerDb(new Float32Array([0, 0])).meanDb).toBe(Number.NEGATIVE_INFINITY);
  });
});
