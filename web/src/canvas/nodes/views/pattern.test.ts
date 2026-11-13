import { describe, expect, it } from "vitest";
import { amplitudePercent, angleGap, patternPath, weightTitle } from "./pattern";

describe("lane weights", () => {
  it("normalises weight bars to the strongest lane", () => {
    expect(
      amplitudePercent([
        { amplitude_db: 0, phase_deg: 0 },
        { amplitude_db: -6.0206, phase_deg: 10 },
        { amplitude_db: -20, phase_deg: 20 },
      ]),
    ).toEqual([100, 50, 10]);
    expect(amplitudePercent([])).toEqual([]);
  });

  it("titles a weight with lane, level and phase", () => {
    expect(weightTitle(0, { amplitude_db: -1.24, phase_deg: 36.6 })).toBe("L1 -1.2 dB 37°");
  });
});

describe("beam pattern", () => {
  it("measures the gap between two angles round the circle", () => {
    expect(angleGap(355, 3)).toBe(8);
    expect(angleGap(10, 190)).toBe(180);
  });

  it("closes the pattern path", () => {
    const path = patternPath([255, 128, 0, 128], 50, 60);
    expect(path.startsWith("M60.0 10.0")).toBe(true);
    expect(path.endsWith("Z")).toBe(true);
    expect(patternPath([], 50, 60)).toBe("");
  });
});
