import { describe, expect, it } from "vitest";
import {
  noiseFigureSummary,
  perSummary,
  sidDeviation,
  sidSummary,
  sidTrends,
  sparkPoints,
} from "./measurement";

function sidAt(level_dbfs: number) {
  return {
    window_ms: 5000,
    floor_dbfs_hz: -110,
    stations: [{ call: "NAA", frequency_hz: 24_000, level_dbfs, snr_db: 30 }],
  };
}

describe("measurement", () => {
  it("summarises a noise figure once both levels are known", () => {
    const report = { window_ms: 1000, level_dbfs: -60.04, settled_s: 3 };
    expect(noiseFigureSummary(report)).toBe("-60.0 dBFS · toggle the noise source");
    expect(noiseFigureSummary({ ...report, y_db: 10.5, nf_db: 4.75 })).toBe(
      "NF 4.75 dB · Y 10.50 dB",
    );
    expect(noiseFigureSummary({ ...report, y_db: 20 })).toBe("Y 20.00 dB, above the ENR");
  });

  it("summarises a PER count", () => {
    expect(
      perSummary({
        received: 990,
        errored: 3,
        missed: 10,
        expected: 1000,
        per: 0.01,
        bits: 256_000,
        bit_errors: 80,
        ber: 3.125e-4,
      }),
    ).toBe("PER 1.00% · 990/1000 · BER 3.1e-4");
    expect(
      perSummary({
        received: 5,
        errored: 0,
        missed: 0,
        expected: 5,
        per: 0,
        bits: 1280,
        bit_errors: 0,
        ber: 0,
      }),
    ).toBe("PER 0.00% · 5/5 · BER 0");
  });

  it("leads a SID summary with the disturbed stations", () => {
    const station = {
      call: "DHO38",
      frequency_hz: 23_400,
      level_dbfs: -50,
      snr_db: 40.2,
      baseline_dbfs: -56,
    };
    const report = { window_ms: 5000, floor_dbfs_hz: -110, stations: [station] };
    expect(sidDeviation(station)).toBe(6);
    expect(sidSummary(report)).toBe("DHO38 40 dB");
    expect(sidSummary({ ...report, stations: [{ ...station, disturbed: true }] })).toBe(
      "SID · DHO38 +6.0 dB",
    );
    expect(sidSummary({ ...report, stations: [] })).toBe("no VLF station in view");
    expect(sidSummary({ ...report, stations: [{ ...station, snr_db: -0.3 }] })).toBe("DHO38 0 dB");
  });

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
