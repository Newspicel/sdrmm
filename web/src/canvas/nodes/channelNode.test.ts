import { describe, expect, it } from "vitest";
import { resolvedNodeOf, resolvedPatch } from "../../test/resolved";
import { lockedChannels, unboundOf } from "./channelNode";

describe("unboundOf", () => {
  it("says why a decoder does not run, from the reason the server served", () => {
    expect(unboundOf({ runs: "unbound", reason: "radio_closed" })).toEqual({
      status: "radio closed",
      hint: "Its radio is not open",
      action: "Open radio",
    });
    expect(unboundOf({ runs: "unbound", reason: "not_placed" })?.action).toBe("Start channel");
    expect(unboundOf({ runs: "unbound", reason: "radio_absent" })?.action).toBeNull();
    expect(unboundOf({ runs: "unbound", reason: "not_wired" })?.status).toBeNull();
    expect(unboundOf({ runs: "unbound", reason: "stopped" })).toEqual({
      status: "stopped",
      hint: "Start it on the node that feeds this one",
      action: null,
    });
    expect(unboundOf({ runs: "channel", device_set: 1, channel: 2, stream: 0 })).toBeNull();
    expect(unboundOf(undefined)).toBeNull();
  });
});

describe("lockedChannels", () => {
  const resolved = resolvedPatch([
    { ...resolvedNodeOf("nfm", "channel"), held: { by: "hand" } },
    { ...resolvedNodeOf("am", "channel"), held: { by: "wire", node: "scan" } },
    { ...resolvedNodeOf("scan", "scanner"), label: "Scanner" },
    resolvedNodeOf("wfm", "channel"),
  ]);
  const faces = new Map<number, string>([
    [1, "nfm"],
    [2, "am"],
    [3, "wfm"],
    [4, "gone"],
  ]);

  it("says who holds each held channel, as the server resolved it", () => {
    expect([...lockedChannels(resolved, faces)]).toEqual([
      [1, "held, unlock it on its node to tune"],
      [2, "tuned by Scanner"],
    ]);
  });

  it("is empty without a resolved patch", () => {
    expect(lockedChannels(null, faces).size).toBe(0);
  });
});
