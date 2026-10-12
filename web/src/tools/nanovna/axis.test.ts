import { describe, expect, it } from "vitest";
import { frequencyTicks, linearTicks, niceStep } from "./axis";

describe("niceStep", () => {
  it("rounds to 1, 2 or 5 times a power of ten", () => {
    expect(niceStep(10, 10)).toBe(1);
    expect(niceStep(2_000_000, 8)).toBe(200_000);
    expect(niceStep(7, 2)).toBe(5);
    expect(niceStep(0.9, 10)).toBeCloseTo(0.1, 12);
    expect(niceStep(80, 10)).toBe(10);
  });

  it("returns zero for an empty or invalid span", () => {
    expect(niceStep(0, 5)).toBe(0);
    expect(niceStep(Number.NaN, 5)).toBe(0);
    expect(niceStep(-3, 5)).toBe(0);
  });
});

describe("linearTicks", () => {
  it("places ticks on nice values inside the range", () => {
    const ticks = linearTicks(-3.2, 7.9, 5);
    expect(ticks.map((tick) => tick.value)).toEqual([-2, 0, 2, 4, 6]);
    expect(ticks.map((tick) => tick.label)).toEqual(["-2", "0", "2", "4", "6"]);
  });

  it("labels fractional steps without float noise", () => {
    const ticks = linearTicks(1, 2, 10);
    expect(ticks.map((tick) => tick.label)).toEqual([
      "1",
      "1.1",
      "1.2",
      "1.3",
      "1.4",
      "1.5",
      "1.6",
      "1.7",
      "1.8",
      "1.9",
      "2",
    ]);
  });

  it("uses a custom formatter", () => {
    const ticks = linearTicks(0, 10, 2, (value) => `${value} dB`);
    expect(ticks.map((tick) => tick.label)).toEqual(["0 dB", "5 dB", "10 dB"]);
  });

  it("accepts a reversed range", () => {
    expect(linearTicks(10, 0, 2).map((tick) => tick.value)).toEqual([0, 5, 10]);
  });

  it("returns nothing for a degenerate range", () => {
    expect(linearTicks(3, 3, 5)).toEqual([]);
    expect(linearTicks(0, Number.POSITIVE_INFINITY, 5)).toEqual([]);
    expect(linearTicks(Number.NaN, 1, 5)).toEqual([]);
  });
});

describe("frequencyTicks", () => {
  it("labels every tick with one shared unit", () => {
    const ticks = frequencyTicks(13_000_000, 15_000_000, 5);
    expect(ticks.map((tick) => tick.label)).toEqual([
      "13 MHz",
      "13.5 MHz",
      "14 MHz",
      "14.5 MHz",
      "15 MHz",
    ]);
  });

  it("adds only the decimals that keep neighbours apart", () => {
    const ticks = frequencyTicks(14_095_000, 14_105_000, 2);
    expect(ticks.map((tick) => tick.label)).toEqual(["14.095 MHz", "14.1 MHz", "14.105 MHz"]);
  });

  it("switches unit with the frequency", () => {
    expect(frequencyTicks(0, 900, 2).map((tick) => tick.label)).toEqual(["0 Hz", "500 Hz"]);
    expect(frequencyTicks(0, 1_000, 2).map((tick) => tick.label)).toEqual([
      "0 kHz",
      "0.5 kHz",
      "1 kHz",
    ]);
    expect(frequencyTicks(100_000, 900_000, 8).at(-1)?.label).toBe("900 kHz");
    expect(frequencyTicks(1e9, 3e9, 2).map((tick) => tick.label)).toEqual([
      "1 GHz",
      "2 GHz",
      "3 GHz",
    ]);
  });

  it("keeps ticks inside the sweep", () => {
    const ticks = frequencyTicks(50_000, 900_000_000, 8);
    expect(ticks[0]?.value).toBeGreaterThanOrEqual(50_000);
    expect(ticks.at(-1)?.value).toBeLessThanOrEqual(900_000_000);
    expect(ticks.map((tick) => tick.label)).toContain("500 MHz");
  });

  it("returns nothing for a single frequency", () => {
    expect(frequencyTicks(14e6, 14e6, 5)).toEqual([]);
    expect(frequencyTicks(Number.NaN, 14e6, 5)).toEqual([]);
  });
});
