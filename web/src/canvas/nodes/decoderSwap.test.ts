import { describe, expect, it, vi } from "vitest";
import type { ChannelDescriptor } from "../../lib/types";
import { analogModes, nextAnalogMode, retypeNode } from "./decoderSwap";

function type(type_id: string, analog_voice: boolean): ChannelDescriptor {
  return { type_id, name: type_id, analog_voice, bandwidth_hz: 1, input_rate_hz: 1 };
}

const TYPES = [type("one", true), type("data", false), type("two", true), type("three", true)];

describe("nextAnalogMode", () => {
  it("rings through the analog voice descriptors in registry order", () => {
    expect(analogModes(TYPES)).toEqual(["one", "two", "three"]);
    expect(nextAnalogMode("one", 1, TYPES)).toBe("two");
    expect(nextAnalogMode("one", -1, TYPES)).toBe("three");
    expect(nextAnalogMode("three", 1, TYPES)).toBe("one");
  });

  it("lands on the first mode from a decoder that is not analog", () => {
    expect(nextAnalogMode("data", 1, TYPES)).toBe("one");
    expect(nextAnalogMode("data", -1, TYPES)).toBe("one");
  });

  it("has nowhere to go without analog descriptors", () => {
    expect(nextAnalogMode("data", 1, [type("data", false)])).toBeNull();
  });
});

describe("retypeNode", () => {
  it("asks the server to retype the node, which decides what survives", async () => {
    const act = vi.fn(async () => ({ result: "retyped" as const, with: { dropped: [] } }));

    await retypeNode("ch", "two", act);

    expect(act).toHaveBeenCalledWith("ch", { action: "retype", with: { channel_type: "two" } });
  });
});
