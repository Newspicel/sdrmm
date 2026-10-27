import { describe, expect, it, vi } from "vitest";
import { sidTrends, sparkPoints } from "./measurement";

function sidAt(level_dbfs: number) {
  return {
    window_ms: 5000,
    floor_dbfs_hz: -110,
    stations: [{ call: "NAA", frequency_hz: 24_000, level_dbfs, snr_db: 30 }],
  };
}

function sidLog(reports: number, stations: number) {
  const listed = Array.from({ length: stations }, (_, k) => ({
    call: `S${k}`,
    frequency_hz: 24_000 + k,
    level_dbfs: -50 - k,
    snr_db: 30,
  }));
  return Array.from({ length: reports }, (_, k) => ({
    at: new Date(k * 1_000).toISOString(),
    report: { window_ms: 5000, floor_dbfs_hz: -110, stations: listed },
  }));
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

describe("sidTrends over a long log", () => {
  it("appends to each trend instead of rebuilding it per record", () => {
    const set = vi.spyOn(Map.prototype, "set");
    const trends = sidTrends(sidLog(500, 10));
    const stores = set.mock.calls.length;
    set.mockRestore();

    expect(trends.size).toBe(10);
    expect(trends.get("S0:24000")).toHaveLength(500);
    expect(stores).toBe(10);
  });
});
