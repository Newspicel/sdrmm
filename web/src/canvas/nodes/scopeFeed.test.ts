import { describe, expect, it } from "vitest";
import type { SpectrumFrame } from "../../lib/frame";
import { displayWindow, metaOf, SCOPE_PORT } from "./scopeFeed";

const FRAME = {
  centerHz: 100_000_000,
  spanHz: 2_000_000,
  dbMin: -95,
  dbMax: -25,
  floorDb: -92,
  bins: new Uint8Array(8),
} as unknown as SpectrumFrame;

describe("scope feed", () => {
  it("taps the IQ port the scope face draws", () => {
    expect(SCOPE_PORT).toBe("iq");
  });

  it("reads the frame's own range until a manual one is held", () => {
    const meta = metaOf(FRAME);
    expect(meta).toEqual({
      centerHz: 100_000_000,
      spanHz: 2_000_000,
      dbMin: -95,
      dbMax: -25,
      floorDb: -92,
    });
    expect(displayWindow(meta, null)).toEqual({ min: -95, max: -25 });
    expect(displayWindow(meta, { min: -80, max: -40 })).toEqual({ min: -80, max: -40 });
  });

  it("falls back to a readable range before the first frame", () => {
    expect(displayWindow(null, null)).toEqual({ min: -100, max: -20 });
  });
});
