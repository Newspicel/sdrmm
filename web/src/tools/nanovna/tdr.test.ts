import { describe, expect, it } from "vitest";
import type { NanoVnaPoint } from "../../lib/types";
import { fft, isHarmonicGrid, timeDomain } from "./tdr";
import { point } from "./testdata";

const SPEED_OF_LIGHT = 299_792_458;

function lineSweep(
  frequencies: readonly number[],
  lengthM: number,
  velocityFactor: number,
  termination: 1 | -1 = 1,
): NanoVnaPoint[] {
  const delay = (2 * lengthM) / (velocityFactor * SPEED_OF_LIGHT);
  return frequencies.map((frequencyHz) => {
    const phase = -2 * Math.PI * frequencyHz * delay;
    return point(frequencyHz, {
      re: termination * Math.cos(phase),
      im: termination * Math.sin(phase),
    });
  });
}

function grid(startHz: number, stepHz: number, count: number): number[] {
  return Array.from({ length: count }, (_, index) => startHz + index * stepHz);
}

function stepAt(result: ReturnType<typeof timeDomain>, distanceM: number): number {
  const index = result.distanceM.findIndex((distance) => distance >= distanceM);
  return result.step?.[index] ?? Number.NaN;
}

describe("fft", () => {
  it("matches a hand computed DFT", () => {
    const re = Float64Array.from([1, 2, 3, 4]);
    const im = new Float64Array(4);
    fft(re, im, false);
    [10, -2, -2, -2].forEach((value, index) => expect(re[index]).toBeCloseTo(value, 12));
    [0, 2, 0, -2].forEach((value, index) => expect(im[index]).toBeCloseTo(value, 12));
  });

  it("returns the input after a forward and inverse transform", () => {
    const original = Array.from({ length: 64 }, (_, index) => Math.sin(index * 0.7) + index / 9);
    const re = Float64Array.from(original);
    const im = Float64Array.from(original, (value) => value / 3);
    fft(re, im, false);
    fft(re, im, true);
    re.forEach((value, index) => expect(value).toBeCloseTo(original[index] ?? 0, 10));
    im.forEach((value, index) => expect(value).toBeCloseTo((original[index] ?? 0) / 3, 10));
  });

  it("refuses a size that is not a power of two", () => {
    expect(() => fft(new Float64Array(6), new Float64Array(6), false)).toThrow(RangeError);
  });
});

describe("isHarmonicGrid", () => {
  it("accepts a grid whose start equals its step", () => {
    expect(isHarmonicGrid(lineSweep(grid(1e6, 1e6, 101), 1, 1))).toBe(true);
    expect(isHarmonicGrid(lineSweep(grid(1.005e6, 1e6, 101), 1, 1))).toBe(true);
  });

  it("rejects an offset grid and a single point", () => {
    expect(isHarmonicGrid(lineSweep(grid(50e6, 0.5e6, 201), 1, 1))).toBe(false);
    expect(isHarmonicGrid(lineSweep([1e6], 1, 1))).toBe(false);
  });
});

describe("timeDomain", () => {
  it("finds an open line in low-pass mode", () => {
    const result = timeDomain(lineSweep(grid(1e6, 1e6, 101), 10, 0.66), 0.66);
    expect(result.lowpass).toBe(true);
    expect(result.distanceM).toHaveLength(512);
    expect(Math.abs((result.peak?.distanceM ?? 0) - 10)).toBeLessThanOrEqual(result.resolutionM);
    expect(result.peak?.value).toBeGreaterThan(0.5);
    expect(result.rangeM).toBeCloseTo((SPEED_OF_LIGHT * 0.66) / 2e6, 6);
  });

  it("turns the step response into impedance", () => {
    const open = timeDomain(lineSweep(grid(1e6, 1e6, 101), 10, 0.66), 0.66);
    expect(stepAt(open, 5)).toBeGreaterThan(45);
    expect(stepAt(open, 5)).toBeLessThan(55);
    expect(stepAt(open, 15)).toBeGreaterThan(1000);
    const short = timeDomain(lineSweep(grid(1e6, 1e6, 101), 10, 0.66, -1), 0.66);
    expect(short.peak?.value).toBeLessThan(-0.5);
    expect(stepAt(short, 15)).toBeLessThan(5);
  });

  it("finds an open line in band-pass mode", () => {
    const result = timeDomain(lineSweep(grid(50e6, 0.5e6, 201), 7, 0.8), 0.8, { window: "hann" });
    expect(result.lowpass).toBe(false);
    expect(result.step).toBeNull();
    expect(Math.abs((result.peak?.distanceM ?? 0) - 7)).toBeLessThanOrEqual(result.resolutionM);
    expect(result.peak?.value).toBeGreaterThan(0.5);
  });

  it("honours the padding request up to the cap", () => {
    const points = lineSweep(grid(1e6, 1e6, 101), 10, 0.66);
    expect(timeDomain(points, 0.66, { padTo: 3000 }).impulse).toHaveLength(4096);
    expect(timeDomain(points, 0.66, { padTo: 1e9 }).impulse).toHaveLength(65_536);
    const rectangular = timeDomain(points, 0.66, { window: "rectangular" });
    expect(Math.abs((rectangular.peak?.distanceM ?? 0) - 10)).toBeLessThanOrEqual(
      rectangular.resolutionM,
    );
  });

  it("returns an empty result for unusable input", () => {
    expect(timeDomain([], 0.66).peak).toBeNull();
  });

  it("finds an open port at zero metres, not its mirror at the end of the range", () => {
    const points = Array.from({ length: 401 }, (_, index) =>
      point(1_000_000 + index * 72_500, { re: 0.99, im: -0.01 }),
    );
    const result = timeDomain(points, 0.66);
    expect(result.peak?.distanceM ?? Number.POSITIVE_INFINITY).toBeLessThanOrEqual(
      2 * result.resolutionM,
    );
    expect(timeDomain(lineSweep(grid(1e6, 1e6, 11), 1, 1), 0).impulse).toEqual([]);
  });
});
