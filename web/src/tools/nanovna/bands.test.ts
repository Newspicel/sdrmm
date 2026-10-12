import { describe, expect, it } from "vitest";
import type { BandAllocation } from "../../lib/types";
import { type PointReadout, readouts } from "./analysis";
import { GERMAN_AMATEUR_PLAN } from "./bandPlanFixture";
import { bandFits, bandsWithin, hamBands } from "./bands";
import { point, resonantSweep } from "./testdata";

const BANDS = hamBands(GERMAN_AMATEUR_PLAN);

function allocation(overrides: Partial<BandAllocation>): BandAllocation {
  return {
    id: "test",
    layer: "world",
    service: "amateur",
    name: "test",
    official_name: "test",
    start_hz: 0,
    stop_hz: 1,
    ...overrides,
  };
}

describe("hamBands", () => {
  it("merges every layer into one range per band", () => {
    expect(BANDS).toEqual([
      { label: "2200 m", startHz: 135_700, stopHz: 137_800 },
      { label: "160 m", startHz: 1_800_000, stopHz: 2_000_000 },
      { label: "80 m", startHz: 3_500_000, stopHz: 3_800_000 },
      { label: "60 m", startHz: 5_351_500, stopHz: 5_366_500 },
      { label: "20 m", startHz: 14_000_000, stopHz: 14_350_000 },
      { label: "23 cm", startHz: 1_240_000_000, stopHz: 1_300_000_000 },
      { label: "3.4 GHz", startHz: 3_400_000_000, stopHz: 3_475_000_000 },
      { label: "24 GHz", startHz: 24_000_000_000, stopHz: 24_250_000_000 },
    ]);
  });

  it("ignores other services", () => {
    const plan = {
      ...GERMAN_AMATEUR_PLAN,
      allocations: [
        ...GERMAN_AMATEUR_PLAN.allocations,
        allocation({ service: "broadcast", start_hz: 3_800_000, stop_hz: 5_400_000 }),
      ],
    };
    expect(hamBands(plan)).toEqual(BANDS);
  });

  it("prefers the shortest wavelength alias", () => {
    const bands = hamBands({
      ...GERMAN_AMATEUR_PLAN,
      allocations: [
        allocation({ start_hz: 10, stop_hz: 20, aliases: ["10.5 m", "10 m cw"] }),
        allocation({ start_hz: 15, stop_hz: 30, aliases: ["10 m", "28 mhz"] }),
        allocation({ start_hz: 30, stop_hz: 40 }),
      ],
    });
    expect(bands).toEqual([{ label: "10 m", startHz: 10, stopHz: 40 }]);
  });
});

describe("bandsWithin", () => {
  it("keeps bands that overlap the range", () => {
    expect(bandsWithin(BANDS, 3_000_000, 10_000_000).map((band) => band.label)).toEqual([
      "80 m",
      "60 m",
    ]);
    expect(bandsWithin(BANDS, 14_300_000, 14_400_000).map((band) => band.label)).toEqual(["20 m"]);
    expect(bandsWithin(BANDS, 15_000_000, 16_000_000)).toEqual([]);
  });
});

describe("bandFits", () => {
  it("rates only bands the sweep touches", () => {
    const fits = bandFits(readouts(resonantSweep(201).points), BANDS);
    expect(fits).toHaveLength(1);
    const fit = fits[0];
    expect(fit?.band.label).toBe("20 m");
    expect(fit?.points).toBe(36);
    expect(fit?.bestVswr).toBeCloseTo(1, 6);
    expect(fit?.bestHz).toBe(14_100_000);
    expect(fit?.worstVswr).toBeGreaterThan(fit?.bestVswr ?? 0);
  });

  it("keeps an infinite worst VSWR but never picks it as best", () => {
    const rows: PointReadout[] = readouts([
      point(3_600_000, { re: 1, im: 0 }),
      point(3_700_000, { re: 0.5, im: 0 }),
      point(3_750_000, { re: 0.6, im: 0 }),
    ]);
    const [fit] = bandFits(rows, BANDS);
    expect(fit?.bestVswr).toBeCloseTo(3, 6);
    expect(fit?.bestHz).toBe(3_700_000);
    expect(fit?.worstVswr).toBe(Number.POSITIVE_INFINITY);
  });

  it("reports an infinite best VSWR when nothing in the band is finite", () => {
    const [fit] = bandFits(readouts([point(3_600_000, { re: 1, im: 0 })]), BANDS);
    expect(fit?.bestVswr).toBe(Number.POSITIVE_INFINITY);
    expect(fit?.bestHz).toBe(3_600_000);
    expect(fit?.points).toBe(1);
  });
});
