import { describe, expect, it } from "vitest";
import { frameFor, onPlotX } from "./Plot";

const BAND = { low: 1e6, high: 30e6 };

describe("onPlotX", () => {
  it("holds a pointer in the axis gutter to the plotted band", () => {
    expect(onPlotX(0, 400)).toBe(52);
    expect(onPlotX(1_000, 400)).toBe(386);
    expect(onPlotX(200, 400)).toBe(200);
  });
});

describe("a zoom drag that starts over the gutter", () => {
  it("never reaches past the measured band", () => {
    const frame = frameFor(400, 200, BAND, { low: 1, high: 4 });
    const low = frame.invertX(onPlotX(-30, 400));
    const high = frame.invertX(onPlotX(900, 400));

    expect(low).toBeCloseTo(BAND.low, 3);
    expect(high).toBeCloseTo(BAND.high, 3);
    expect(frame.invertX(-30)).toBeLessThan(BAND.low);
    expect(frame.invertX(900)).toBeGreaterThan(BAND.high);
  });
});
