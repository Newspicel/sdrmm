import { describe, expect, it } from "vitest";
import type { RangeDopplerFrame } from "../../../lib/frame";
import {
  dopplerHzAt,
  frameAxes,
  hoverText,
  pxToSurface,
  rangeKmAt,
  surfaceToPx,
  velocityMps,
} from "./rangeDoppler";

function frame(overrides: Partial<RangeDopplerFrame> = {}): RangeDopplerFrame {
  return {
    streamId: 1,
    seq: 1,
    timestamp: 0n,
    ranges: 100,
    dopplers: 65,
    rangeFirstM: 0,
    rangeStepM: 150,
    dopplerFirstHz: -64,
    dopplerStepHz: 2,
    carrierHz: 100e6,
    dbMin: -3,
    dbMax: 30,
    cells: new Uint8Array(100 * 65),
    ...overrides,
  };
}

describe("range-Doppler axes", () => {
  it("converts bins, rows and Doppler to km, Hz and m/s", () => {
    const axes = frameAxes(frame());
    expect(rangeKmAt(10, axes)).toBeCloseTo(1.5);
    expect(dopplerHzAt(0, axes)).toBe(-64);
    expect(dopplerHzAt(32, axes)).toBe(0);
    expect(velocityMps(10, 100e6)).toBeCloseTo(-29.98, 2);
    expect(velocityMps(10, 0)).toBeNull();
  });

  it("places a point on the plot", () => {
    const axes = frameAxes(frame());
    const box = { w: 400, h: 260 };
    expect(surfaceToPx({ rangeKm: 0, dopplerHz: 0 }, axes, box).y).toBeCloseTo(130);
    const far = surfaceToPx({ rangeKm: rangeKmAt(99, axes), dopplerHz: 64 }, axes, box);
    expect(far.x).toBeCloseTo(398);
    expect(far.y).toBeCloseTo(2);
    const back = pxToSurface(far.x, far.y, axes, box);
    expect(back.rangeKm).toBeCloseTo(rangeKmAt(99, axes));
    expect(back.dopplerHz).toBeCloseTo(64);
  });

  it("reads the hover point with sign and speed", () => {
    expect(hoverText({ rangeKm: 42.14, dopplerHz: -35 }, 100e6, 12.4)).toBe(
      "42.1 km · -35 Hz · +105 m/s · 12 dB",
    );
    expect(hoverText({ rangeKm: 1, dopplerHz: 3 }, 0, null)).toBe("1.0 km · +3 Hz");
  });
});
