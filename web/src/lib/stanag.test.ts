import { describe, expect, it } from "vitest";
import { stanagSummary } from "./stanag";

describe("stanagSummary", () => {
  it("names the modem, rate and carrier", () => {
    expect(
      stanagSummary({
        modem: "mil_std_188_110a",
        confidence: 0.9,
        audio_hz: 1_801.4,
        bandwidth_hz: 2_700,
        rate: { bps: 1_200, interleave: "long", voice: false },
      }),
    ).toBe("MIL-STD-188-110A · 1200 bps long · 1801 Hz");
  });

  it("says when the modem is gone", () => {
    expect(stanagSummary({ confidence: 0, audio_hz: 0, bandwidth_hz: 0 })).toBe("modem gone");
  });
});
