import { describe, expect, it } from "vitest";
import { RELATIVE_MARKS, spectrumPath, TRUE_MARKS } from "../../../components/Rose";
import type { BearingView } from "../../../lib/types";
import {
  frameOptions,
  frameRotation,
  NEEDS_HEADING,
  roseLetters,
  roseNeedles,
  roseWedge,
  shownFrame,
} from "./bearing";

function bearing(overrides: Partial<BearingView> = {}): BearingView {
  return {
    spectrum: [],
    heading_deg: 90,
    needles: [
      { relative_deg: 47, true_deg: 137, weight: "primary" },
      { relative_deg: 313, true_deg: 43, weight: "mirror" },
      { relative_deg: 200, true_deg: null, weight: "secondary" },
    ],
    sigma_deg: 3.1,
    ...overrides,
  };
}

describe("bearing frame", () => {
  it("rotates the spectrum by the heading", () => {
    const view = bearing();
    expect(frameRotation(view, "true")).toBe(90);
    expect(frameRotation(view, "relative")).toBe(0);
    const spectrum = [255, 0, 0, 0];
    expect(spectrumPath(spectrum, 50, 10, 40, frameRotation(view, "true"))).toMatch(
      /^M90\.00 50\.00/,
    );
  });

  it("falls back to the array frame without a heading", () => {
    expect(roseLetters(true)).toBe(TRUE_MARKS);
    expect(roseLetters(false)).toBe(RELATIVE_MARKS);
    expect(shownFrame(null, true)).toBe("true");
    expect(shownFrame("relative", true)).toBe("relative");
    expect(shownFrame("true", false)).toBe("relative");
    const unavailable = frameOptions(false).find((option) => option.value === "true");
    expect(unavailable).toMatchObject({ disabled: true, title: NEEDS_HEADING });
  });

  it("reads each needle in the chosen frame", () => {
    expect(roseNeedles(bearing(), "true")).toEqual([
      { deg: 137, weight: "primary" },
      { deg: 43, weight: "mirror" },
    ]);
    expect(roseNeedles(bearing(), "relative")).toHaveLength(3);
    expect(roseWedge(bearing(), "relative")).toEqual({ deg: 47, sigmaDeg: 3.1 });
    expect(roseWedge(bearing({ needles: [] }), "true")).toBeNull();
    expect(roseWedge(null, "true")).toBeNull();
  });
});
