import { describe, expect, it } from "vitest";
import { ismSurveySummary, percentOf, wifiOccupancySummary } from "./airtime";

describe("airtime", () => {
  it("keeps a decimal below ten percent", () => {
    expect(percentOf(0.004)).toBe("0.4%");
    expect(percentOf(0.314)).toBe("31%");
  });

  it("summarises reports", () => {
    expect(
      wifiOccupancySummary({
        window_ms: 1000,
        measured: 1,
        channels: [{ band: "ghz2_4", number: 6, centre_hz: 2_437e6, busy: 0.314 }],
      }),
    ).toBe("ch 6 31%");
    expect(wifiOccupancySummary({ window_ms: 1000, measured: 1, channels: [] })).toBe(
      "no Wi-Fi channel in view",
    );
    expect(
      ismSurveySummary({
        window_ms: 1000,
        low_hz: 0,
        high_hz: 1,
        measured: 1,
        busy: 0.42,
        kinds: [{ kind: "bluetooth", bursts: 12, airtime: 0.004, mean_us: 376, peak_dbfs: -30 }],
        dropped: 0,
      }),
    ).toBe("42% busy · Bluetooth 12 · 0.4%");
  });
});
