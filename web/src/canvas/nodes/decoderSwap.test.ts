import { describe, expect, it, vi } from "vitest";
import { ANALOG_MODES, nextAnalogMode, retypeNode } from "./decoderSwap";

describe("nextAnalogMode", () => {
  it("walks the ring in both directions", () => {
    expect(nextAnalogMode("nfm", 1)).toBe("wfm");
    expect(nextAnalogMode("nfm", -1)).toBe("ssb");
    expect(nextAnalogMode("ssb", 1)).toBe("nfm");
  });

  it("lands on the first mode from a decoder that is not analog", () => {
    expect(nextAnalogMode("adsb", 1)).toBe(ANALOG_MODES[0]);
    expect(nextAnalogMode("adsb", -1)).toBe(ANALOG_MODES[0]);
  });
});

describe("retypeNode", () => {
  it("asks the server to retype the node, which decides what survives", async () => {
    const act = vi.fn(async () => ({ result: "retyped" as const, with: { dropped: [] } }));

    await retypeNode("ch", "am", act);

    expect(act).toHaveBeenCalledWith("ch", { action: "retype", with: { channel_type: "am" } });
  });
});
