import { describe, expect, it } from "vitest";
import type { DeviceSet, PatchGraph, PatchNode } from "../lib/types";
import { resolvedNodeOf, resolvedPatch, resolvedPort } from "../test/resolved";
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

const RESOLVED = resolvedPatch([
  resolvedNodeOf("device:1", "device", [], { runs: "radio", device_set: 1 }),
  resolvedNodeOf("channel:1", "channel", [
    resolvedPort(
      { name: "iq", port_type: "iq", direction: "in" },
      {
        feeds: [{ source: "device:1", port: "iq", tap: { tap: "lane", device_set: 1, stream: 0 } }],
      },
    ),
  ]),
]);

const ONE = new Map([["device:1", set(1)]]);
const TWO = new Map([
  ["device:1", set(1)],
  ["device:2", set(2)],
]);

describe("libraryTarget", () => {
  it("takes the selected device", () => {
    expect(libraryTarget(GRAPH, TWO, null, "device:2")).toEqual({
      kind: "device",
      node: "device:2",
      set: set(2),
      locked: false,
    });
  });

  it("takes a selected channel with the radio it is wired to", () => {
    expect(libraryTarget(GRAPH, TWO, RESOLVED, "channel:1")).toEqual({
      kind: "channel",
      node: "channel:1",
      set: set(1),
      locked: false,
    });
  });

  it("keeps an unwired channel as a target without a radio", () => {
    expect(libraryTarget(GRAPH, TWO, RESOLVED, "channel:2")).toEqual({
      kind: "channel",
      node: "channel:2",
      set: null,
      locked: true,
    });
  });

  it("falls back to the only device when nothing is selected", () => {
    expect(libraryTarget(GRAPH, ONE, null, null)?.node).toBe("device:1");
    expect(libraryTarget(GRAPH, ONE, null, "speaker:1")?.node).toBe("device:1");
  });

  it("has no target when several devices are drawn and none is selected", () => {
    expect(libraryTarget(GRAPH, TWO, null, null)).toBeNull();
    expect(libraryTarget(GRAPH, new Map(), null, null)).toBeNull();
  });
});
