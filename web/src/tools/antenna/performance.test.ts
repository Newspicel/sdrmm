import { describe, expect, it } from "vitest";
import type { AntennaReport, AntennaSimulation, AntennaSweepPoint } from "../../lib/types";
import {
  bestMatching,
  matchings,
  pointsOf,
  reflection,
  resonances,
  roundedHz,
  simulationOf,
  sweepWindow,
  swrOf,
  swrSpan,
  throughLine,
  unavailableReason,
  wideWindow,
} from "./performance";

function sweep(rows: [number, number, number][]): AntennaSweepPoint[] {
  return rows.map(([frequency_hz, re, im]) => ({ frequency_hz, impedance: { re, im } }));
}

const SIMULATION: AntennaSimulation = {
  impedance: { re: 70, im: 0 },
  gain_dbi: 2.1,
  efficiency: 0.99,
  takeoff_deg: 0,
  bearing_deg: 0,
  front_to_back_db: 0,
  polarization: "horizontal",
  azimuth: { step_deg: 2, gain_dbi: [] },
  elevation: { step_deg: 2, gain_dbi: [] },
  sweep: [],
  wires: [],
  feed_wire: 0,
  feed_segment: 0,
  wire_radius_m: 0.001,
};

function report(simulation: AntennaReport["simulation"]): AntennaReport {
  return {
    design: { type: "dipole" },
    frequency_hz: 14_200_000,
    wavelength_m: 21.1,
    velocity_factor: 0.95,
    parts: [],
    geometry: { segments: [], feed: { x_m: 0, y_m: 0, z_m: 0 } },
    balanced: true,
    notes: [],
    simulation,
  };
}

describe("simulationOf", () => {
  it("unwraps a solved model and explains one that was not", () => {
    const solved = report({ status: "solved", ...SIMULATION });
    expect(simulationOf(solved)?.gain_dbi).toBe(2.1);
    expect(unavailableReason(solved)).toBeNull();

    const skipped = report({ status: "unavailable", reason: "Raise it" });
    expect(simulationOf(skipped)).toBeNull();
    expect(unavailableReason(skipped)).toBe("Raise it");
    expect(simulationOf(null)).toBeNull();
  });
});

describe("reflection and SWR", () => {
  it("is zero for the reference and grows with the mismatch", () => {
    expect(reflection({ re: 50, im: 0 })).toEqual({ re: 0, im: 0 });
    expect(swrOf({ re: 50, im: 0 })).toBeCloseTo(1, 9);
    expect(swrOf({ re: 100, im: 0 })).toBeCloseTo(2, 9);
    expect(swrOf({ re: 25, im: 0 })).toBeCloseTo(2, 9);
    expect(swrOf({ re: 0, im: 50 })).toBe(Number.POSITIVE_INFINITY);
  });

  it("turns a sweep into points the NanoVNA views read", () => {
    const points = pointsOf(sweep([[14_000_000.4, 50, 0]]));
    expect(points[0]?.frequency_hz).toBe(14_000_000);
    expect(points[0]?.s11.re).toBeCloseTo(0, 12);
    expect(points[0]?.s21).toEqual({ re: 0, im: 0 });
  });
});

describe("matchings", () => {
  it("picks the transformer that brings an end-fed wire to 50 Ω", () => {
    const z = { re: 2450, im: 0 };
    const best = bestMatching(matchings(z, 7e6, false), z, 7e6);
    expect(best?.label).toBe("49:1 unun");
    expect(swrOf(best?.apply(z, 7e6) ?? z)).toBeCloseTo(1, 2);
  });

  it("calls the transformer a balun on a balanced feedpoint", () => {
    const labels = matchings({ re: 300, im: 0 }, 14e6, true).map((entry) => entry.label);
    expect(labels).toContain("4:1 balun");
  });

  it("offers an L network that reaches 50 Ω at the design frequency", () => {
    const z = { re: 20, im: -30 };
    const list = matchings(z, 14e6, true);
    const networks = list.filter((entry) => entry.id.startsWith("l-"));
    expect(networks.length).toBeGreaterThan(0);
    for (const network of networks) {
      expect(swrOf(network.apply(z, 14e6))).toBeCloseTo(1, 3);
    }
  });

  it("keeps the direct feed when nothing does better", () => {
    const z = { re: 50, im: 0 };
    expect(bestMatching(matchings(z, 14e6, true), z, 14e6)?.id).toBe("direct");
  });

  it("matches a resistive load through a quarter-wave line at the design frequency", () => {
    const z = { re: 100, im: 0 };
    const line = matchings(z, 14e6, true).find((entry) => entry.id === "quarter-wave");
    expect(line).toBeDefined();
    expect(swrOf(line?.apply(z, 14e6) ?? z)).toBeCloseTo(1, 6);
  });
});

describe("throughLine", () => {
  it("passes a matched line straight through at any length", () => {
    const z = throughLine({ re: 75, im: 0 }, 75, 0.7);
    expect(z.re).toBeCloseTo(75, 9);
    expect(z.im).toBeCloseTo(0, 9);
  });
});

describe("swrSpan", () => {
  const rows = sweep([
    [13e6, 150, 0],
    [14e6, 75, 0],
    [15e6, 50, 0],
    [16e6, 75, 0],
    [17e6, 150, 0],
  ]);

  it("measures where the SWR stays under 2 around the design frequency", () => {
    const span = swrSpan(rows, (z) => z, 15e6);
    expect(span?.lowHz).toBeCloseTo(13_666_667, -1);
    expect(span?.highHz).toBeCloseTo(16_333_333, -1);
    expect(span?.openLow).toBe(false);
    expect(span?.openHigh).toBe(false);
  });

  it("has no span when the design frequency is already over 2", () => {
    expect(swrSpan(rows, (z) => z, 13e6)).toBeNull();
  });

  it("says when the span runs off the sweep", () => {
    const span = swrSpan(rows.slice(1, 3), (z) => z, 15e6);
    expect(span?.openLow).toBe(true);
    expect(span?.openHigh).toBe(true);
  });
});

describe("resonances", () => {
  it("tells series from parallel by which way the reactance crosses", () => {
    const found = resonances(
      sweep([
        [10e6, 20, -100],
        [12e6, 60, 100],
        [14e6, 2000, 400],
        [16e6, 1800, -400],
      ]),
    );
    expect(found).toHaveLength(2);
    expect(found[0]?.kind).toBe("series");
    expect(found[0]?.frequencyHz).toBeCloseTo(11e6, -3);
    expect(found[0]?.resistanceOhms).toBeCloseTo(40, 6);
    expect(found[1]?.kind).toBe("parallel");
    expect(found[1]?.frequencyHz).toBeCloseTo(15e6, -3);
  });
});

describe("sweep windows", () => {
  it("covers the band around the design frequency with room either side", () => {
    const window = sweepWindow(14.2e6, [{ label: "20m", startHz: 14e6, stopHz: 14.35e6 }], 41);
    expect(window.startHz).toBeLessThanOrEqual(14e6 - 0.175e6);
    expect(window.stopHz).toBeGreaterThanOrEqual(14.35e6 + 0.175e6);
    expect(window.startHz).toBe(13_520_000);
  });

  it("keeps a few percent either side outside the bands, on round steps", () => {
    expect(sweepWindow(100e6, [], 41)).toEqual({ startHz: 95e6, stopHz: 105e6, points: 41 });
    const odd = sweepWindow(145.5e6, [], 41);
    const step = (odd.stopHz - odd.startHz) / (odd.points - 1);
    expect(step).toBe(400_000);
    expect(odd.startHz % step).toBe(0);
  });

  it("goes wide enough to find harmonics without leaving the allowed span", () => {
    const wide = wideWindow(7e6, 121);
    expect(wide.startHz).toBeGreaterThanOrEqual(2.8e6);
    expect(wide.stopHz).toBeLessThanOrEqual(22.4e6);
    expect(wide.stopHz / wide.startHz).toBeLessThanOrEqual(8);
    expect(wide.points).toBeGreaterThan(90);
  });
});

describe("roundedHz", () => {
  it("keeps four significant figures", () => {
    expect(roundedHz(14_240_575.031)).toBe(14_240_000);
    expect(roundedHz(710_494.5)).toBe(710_500);
  });
});
