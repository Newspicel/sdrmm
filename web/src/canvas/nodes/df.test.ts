import { describe, expect, it } from "vitest";
import { RELATIVE_MARKS, spectrumPath, TRUE_MARKS } from "../../components/Rose";
import { isStale } from "../../lib/processors";
import type { DfPeak, DfReading } from "../../lib/types";
import {
  bearingLabel,
  dfChips,
  frameOptions,
  frameRotation,
  NEEDS_HEADING,
  peakText,
  roseLetters,
  roseNeedles,
  roseWedge,
  shownFrame,
} from "./df";

function peak(overrides: Partial<DfPeak> = {}): DfPeak {
  return {
    relative_deg: 47,
    true_deg: 137,
    power_db: -40,
    confidence: 0.82,
    sigma_deg: 3.1,
    ...overrides,
  };
}

function dfReading(overrides: Partial<DfReading> = {}): DfReading {
  return {
    at: "2026-09-28T12:00:00Z",
    peaks: [peak()],
    pseudospectrum: [],
    azimuth_deg: 90,
    station: { lat: 52, lon: 13 },
    sources: 1,
    sources_auto: true,
    squelched: false,
    aliasing: false,
    ...overrides,
  };
}

describe("bearing frame", () => {
  it("rotates the spectrum by the array azimuth", () => {
    const reading = dfReading({ azimuth_deg: 90 });
    expect(frameRotation(reading, "true")).toBe(90);
    expect(frameRotation(reading, "relative")).toBe(0);
    const spectrum = [255, 0, 0, 0];
    expect(spectrumPath(spectrum, 50, 10, 40, frameRotation(reading, "true"))).toMatch(
      /^M90\.00 50\.00/,
    );
    expect(spectrumPath(spectrum, 50, 10, 40, frameRotation(reading, "relative"))).toMatch(
      /^M50\.00 10\.00/,
    );
  });

  it("switches letters when the heading goes away", () => {
    expect(roseLetters(true)).toBe(TRUE_MARKS);
    expect(roseLetters(false)).toBe(RELATIVE_MARKS);
    expect(roseLetters(false).map((mark) => mark.label)).toEqual(["F", "R", "B", "L"]);
    expect(shownFrame(null, true)).toBe("true");
    expect(shownFrame("relative", true)).toBe("relative");
    expect(shownFrame("true", false)).toBe("relative");
    const unavailable = frameOptions(false).find((option) => option.value === "true");
    expect(unavailable).toMatchObject({ disabled: true, title: NEEDS_HEADING });
  });

  it("reads each peak in the chosen frame", () => {
    const first = peak({ mirror_deg: 313, mirror_true_deg: 43 });
    const second = peak({ relative_deg: 200, true_deg: 290, confidence: 0.4, sigma_deg: 0.42 });
    expect(peakText(first, "true")).toBe("137°");
    expect(peakText(first, "relative")).toBe("047°");
    expect(peakText(peak({ true_deg: null }), "true")).toBe("-");
    expect(roseNeedles([first, second], "true")).toEqual([
      { deg: 137, weight: "primary" },
      { deg: 43, weight: "mirror" },
      { deg: 290, weight: "secondary" },
    ]);
    expect(roseWedge([first], "relative")).toEqual({ deg: 47, sigmaDeg: 3.1 });
    expect(roseWedge([], "true")).toBeNull();
    expect(bearingLabel(359.7)).toBe("000°");
    expect(bearingLabel(-5)).toBe("355°");
  });
});

describe("dfChips", () => {
  it("lists chips for mirror, aliasing, squelch and stale", () => {
    const flagged = dfReading({ mirror: true, aliasing: true, squelched: true });
    expect(dfChips(flagged, null, true).map((chip) => chip.label)).toEqual([
      "Stale",
      "Squelch",
      "Aliased",
      "Mirror",
    ]);
    expect(dfChips(dfReading(), null, false)).toEqual([]);
    expect(dfChips(null, null, false)).toEqual([]);
  });

  it("has a chip for every flag and the array gate first", () => {
    const everything = dfReading({
      squelched: true,
      azimuth_deg: null,
      station: null,
      aliasing: true,
      mode_aliasing: true,
      mirror: true,
      rotating: true,
      singular: true,
      table_out_of_range: true,
    });
    const chips = dfChips(everything, "phase", false);
    expect(chips.map((chip) => chip.label)).toEqual([
      "Needs cal",
      "Squelch",
      "No heading",
      "No position",
      "Aliased",
      "Mode alias",
      "Mirror",
      "Rotating",
      "Singular",
      "Table off",
    ]);
    expect(chips.every((chip) => chip.title.length > 0)).toBe(true);
    expect(chips[0]?.danger).toBe(true);
  });

  it("calls a reading stale after three report periods", () => {
    expect(isStale(0, 2_999, 1_000)).toBe(false);
    expect(isStale(0, 3_001, 1_000)).toBe(true);
  });
});
