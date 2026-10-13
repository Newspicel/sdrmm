import { describe, expect, it } from "vitest";
import type { PatchGraph, PatchNode } from "../../lib/types";
import { lockedChannels, unboundOf } from "./channelNode";

function node(id: string, body: Partial<PatchNode> & Pick<PatchNode, "kind">): PatchNode {
  return { id, position: { x: 0, y: 0 }, ...body } as PatchNode;
}

const graph: PatchGraph = {
  nodes: [
    node("dev", { kind: "device", data: { device: { backend: "rtlsdr", serial: "A" } } }),
    node("blank", { kind: "device", data: { device: null } }),
    node("nfm", { kind: "channel", data: { channel_type: "nfm" } }),
    node("loose", { kind: "channel", data: { channel_type: "am" } }),
  ],
  edges: [{ from: { node: "dev", port: "iq" }, to: { node: "nfm", port: "iq" } }],
};

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
    expect(unboundOf({ runs: "channel", device_set: 1, channel: 2, stream: 0 })).toBeNull();
    expect(unboundOf(undefined)).toBeNull();
  });
});

describe("lockedChannels", () => {
  const held: PatchGraph = {
    nodes: [
      node("nfm", { kind: "channel", data: { channel_type: "nfm", tuning_locked: true } }),
      node("am", { kind: "channel", data: { channel_type: "am" } }),
      node("trunk", { kind: "dmr_trunk", data: {} }),
    ],
    edges: [],
  };
  const faces = new Map<number, string>([
    [1, "nfm"],
    [2, "am"],
    [3, "trunk"],
    [4, "gone"],
  ]);

  it("collects the live channels whose node holds its frequency", () => {
    expect([...lockedChannels(held, faces)]).toEqual([1]);
  });

  it("is empty when no face is locked", () => {
    expect(lockedChannels(graph, faces).size).toBe(0);
  });
});
