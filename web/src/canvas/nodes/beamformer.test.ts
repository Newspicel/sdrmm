import { describe, expect, it } from "vitest";
import type { BeamformerReading } from "../../lib/types";
import { amplitudePercent, beamChips, patternPath, weightTitle } from "./beamformer";

function reading(overrides: Partial<BeamformerReading> = {}): BeamformerReading {
  return { at: "2026-09-28T12:00:00Z", output_db: -42, nulls_deg: [], weights: [], ...overrides };
}

describe("beamformer weights", () => {
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

describe("beamformer readout", () => {
  it("raises a chip for every flag", () => {
    const labels = beamChips(
      reading({
        no_steer: true,
        steer_stale: true,
        singular: true,
        resets: 2,
        band_full: true,
        steer_deg: 40,
        nulls_deg: [45],
      }),
    ).map((chip) => chip.label);
    expect(labels).toEqual(["No steer", "Stale", "Singular", "Reset", "Band full", "Null close"]);
    expect(
      beamChips(reading({ steer_deg: 355, nulls_deg: [3] })).map((chip) => chip.label),
    ).toEqual(["Null close"]);
    expect(beamChips(null)).toEqual([]);
  });

  it("closes the pattern path", () => {
    const path = patternPath([255, 128, 0, 128], 50, 60);
    expect(path.startsWith("M60.0 10.0")).toBe(true);
    expect(path.endsWith("Z")).toBe(true);
    expect(patternPath([], 50, 60)).toBe("");
  });
});
