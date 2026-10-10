import { describe, expect, it } from "vitest";
import { hsvToRgb, rgbToHsv } from "./hsv";

describe("hsv", () => {
  it("maps primaries to their hues", () => {
    expect(rgbToHsv([1, 0, 0])).toEqual({ h: 0, s: 1, v: 1 });
    expect(rgbToHsv([0, 1, 0])).toEqual({ h: 120, s: 1, v: 1 });
    expect(rgbToHsv([0, 0, 1])).toEqual({ h: 240, s: 1, v: 1 });
  });

  it("gives greys no saturation", () => {
    expect(rgbToHsv([0.5, 0.5, 0.5])).toEqual({ h: 0, s: 0, v: 0.5 });
    expect(rgbToHsv([0, 0, 0])).toEqual({ h: 0, s: 0, v: 0 });
  });

  it("round-trips arbitrary colours", () => {
    for (const rgb of [
      [0.11765, 0.56471, 1],
      [0.8, 0.2, 0.6],
      [0.25, 0.75, 0.1],
    ] as const) {
      const back = hsvToRgb(rgbToHsv(rgb));
      back.forEach((channel, i) => expect(channel).toBeCloseTo(rgb[i] ?? 0, 6));
    }
  });
});
