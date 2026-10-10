import { describe, expect, it } from "vitest";
import type { ChannelInfo, DeviceSet, PatchGraph } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { ChannelFace } from "./ChannelFace";

const RADIO = placed("radio", {
  kind: "device",
  data: { device: { backend: "virtual", key: "band" } },
});
const BLE = placed("ble", { kind: "channel", data: { channel_type: "ble" } });

const GRAPH: PatchGraph = {
  nodes: [RADIO, BLE],
  edges: [{ from: { node: "radio", port: "iq" }, to: { node: "ble", port: "iq" } }],
};

function channel(rejected: number): ChannelInfo {
  return {
    id: 3,
    stream: 0,
    rejected,
    settings: {
      frequency_hz: 2_426_000_000,
      params: { type: "ble", settings: { link: "channel" } },
    } as ChannelInfo["settings"],
  };
}

function face(rejected: number): string {
  const set: DeviceSet = {
    id: 1,
    device: { driver: "virtual", key: "band", label: "Test band" },
    capabilities: {
      freq_ranges: [],
      sample_rates: [],
      gains: [],
      antennas: [],
      bandwidths: [],
      extra: [],
      duplex: "rx_only",
    },
    settings: {},
    status: "running",
    channels: [channel(rejected)],
    overruns: 0,
  };
  return renderFace(ChannelFace, BLE, {
    graph: GRAPH,
    devices: new Map([["radio", set]]),
    channels: new Map([["ble", channel(rejected)]]),
    owners: new Map([["ble", "radio"]]),
  });
}

describe("ChannelFace", () => {
  it("shows packets that synced but failed their check", () => {
    const html = face(12);
    expect(html).toContain('title="Rejected: packets heard but failed their check"');
    expect(html).toContain(">12</b>");
  });

  it("says nothing while every packet reads", () => {
    expect(face(0)).not.toContain("Rejected");
  });
});
