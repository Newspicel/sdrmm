import { describe, expect, it } from "vitest";
import type { SkyCell, SkyScanParams } from "../../lib/types";
import { heatColor, levelSpan, skySlot } from "./skyScan";

const SETTINGS: SkyScanParams = {
  offset_hz: 0,
  bandwidth_hz: 12_500,
  azimuth_from_deg: 300,
  azimuth_span_deg: 120,
  elevation_from_deg: 10,
  elevation_to_deg: 30,
  step_deg: 10,
  dwell_ms: 1_000,
};

function cell(azimuth_deg: number, elevation_deg: number, level_dbfs = -60): SkyCell {
  return { azimuth_deg, elevation_deg, level_dbfs };
}

describe("skySlot", () => {
  it("counts columns clockwise from the start, across north", () => {
    expect(skySlot(SETTINGS, cell(300, 10))).toEqual({ column: 0, row: 0 });
    expect(skySlot(SETTINGS, cell(10, 30))).toEqual({ column: 7, row: 2 });
  });
});

describe("levelSpan", () => {
  it("spans the quietest to the loudest cell", () => {
    expect(levelSpan([])).toBeNull();
    expect(levelSpan([cell(0, 0, -80), cell(10, 0, -40)])).toEqual({ low: -80, high: -40 });
  });
});

describe("heatColor", () => {
  it("tells the loudest from the quietest", () => {
    const span = { low: -80, high: -40 };
    expect(heatColor(-80, span)).not.toEqual(heatColor(-40, span));
    expect(heatColor(-50, { low: -50, high: -50 })).toEqual(heatColor(-40, span));
  });
});
