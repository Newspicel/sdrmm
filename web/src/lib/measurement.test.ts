import { describe, expect, it } from "vitest";
import { sidTrends, sparkPoints } from "./measurement";

function sidAt(level_dbfs: number) {
  return {
    window_ms: 5000,
    floor_dbfs_hz: -110,
    stations: [{ call: "NAA", frequency_hz: 24_000, level_dbfs, snr_db: 30 }],
  };
}

describe("measurement", () => {
  it("orders SID levels into one trend per station", () => {
    const trends = sidTrends([
      { at: "2026-10-11T10:00:05Z", report: sidAt(-48) },
      { at: "2026-10-11T10:00:00Z", report: sidAt(-50) },
    ]);
    expect(trends.get("NAA:24000")).toEqual([-50, -48]);
    expect(sparkPoints([-50, -48], 10, 4)).toBe("0.0,4.0 10.0,0.0");
    expect(sparkPoints([-50], 10, 4)).toBe("");
  });
});
