import { describe, expect, it } from "vitest";
import type { DeviceSet, PatchGraph, PatchNode } from "../lib/types";
import { libraryTarget } from "./libraryTarget";

function set(id: number): DeviceSet {
  return {
    id,
    device: { driver: "rtlsdr", key: String(id), label: "RTL-SDR" },
    capabilities: {
      freq_ranges: [],
      sample_rates: [],
      gains: [],
      antennas: [],
      bandwidths: [],
      duplex: "rx_only",
    },
    settings: {},
    status: "running",
    channels: [],
    overruns: 0,
  };
}

function node(id: string, body: Partial<PatchNode> & Pick<PatchNode, "kind">): PatchNode {
  return { id, position: { x: 0, y: 0 }, ...body } as PatchNode;
}

const GRAPH: PatchGraph = {
  nodes: [
    node("device:1", { kind: "device", data: {} }),
    node("device:2", { kind: "device", data: {} }),
    node("channel:1", { kind: "channel", data: { channel_type: "nfm" } }),
    node("channel:2", { kind: "channel", data: { channel_type: "nfm", tuning_locked: true } }),
    node("speaker:1", { kind: "speaker" }),
  ],
  edges: [{ from: { node: "device:1", port: "iq" }, to: { node: "channel:1", port: "iq" } }],
};

const ONE = new Map([["device:1", set(1)]]);
const TWO = new Map([
  ["device:1", set(1)],
  ["device:2", set(2)],
]);

describe("libraryTarget", () => {
  it("takes the selected device", () => {
    expect(libraryTarget(GRAPH, TWO, "device:2")).toEqual({
      kind: "device",
      node: "device:2",
      set: set(2),
    });
  });

  it("takes a selected channel, wired or not, and leaves locks to the server", () => {
    expect(libraryTarget(GRAPH, TWO, "channel:1")).toEqual({ kind: "channel", node: "channel:1" });
    expect(libraryTarget(GRAPH, TWO, "channel:2")).toEqual({ kind: "channel", node: "channel:2" });
  });

  it("acts on the selected node only, never on the only device", () => {
    expect(libraryTarget(GRAPH, ONE, null)).toBeNull();
    expect(libraryTarget(GRAPH, ONE, "speaker:1")).toBeNull();
  });
});
