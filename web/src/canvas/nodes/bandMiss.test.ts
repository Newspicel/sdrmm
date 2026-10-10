import { describe, expect, it } from "vitest";
import { bandMissOf, bandMissSaid } from "./bandMiss";

describe("bandMissSaid", () => {
  it("names the width a decoder needs, rounded up", () => {
    const said = bandMissSaid({ reason: "too_wide", needs_hz: 7_607_143, top_rate_hz: 20e6 });
    expect(said.label).toBe("needs 7.61 MHz");
    expect(said.title).toContain("Raise its sample rate to 7.61 MHz");
  });

  it("says when the radio cannot reach that width at all", () => {
    const said = bandMissSaid({ reason: "too_wide", needs_hz: 8e6, top_rate_hz: 3.2e6 });
    expect(said.title).toContain("tops out at 3.2 MHz");
  });

  it("tells a crowded decoder from one the radio was tuned away from", () => {
    expect(bandMissSaid({ reason: "crowded" }).label).toBe("crowded out");
    expect(bandMissSaid({ reason: "tuned_away" }).label).toBe("out of band");
    expect(bandMissSaid({ reason: "off_tuner" }).label).toBe("off tuner");
  });
});

describe("bandMissOf", () => {
  it("trusts the engine once it reports", () => {
    expect(bandMissOf({ reported: null, reaches: false })).toBeNull();
    expect(bandMissOf({ reported: { reason: "crowded" }, reaches: true })).toEqual({
      reason: "crowded",
    });
  });

  it("guesses from the window before the engine reports", () => {
    expect(bandMissOf({ reported: undefined, reaches: false })).toEqual({ reason: "tuned_away" });
    expect(bandMissOf({ reported: undefined, reaches: true })).toBeNull();
  });
});
