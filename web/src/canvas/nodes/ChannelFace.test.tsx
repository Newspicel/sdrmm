import { afterEach, describe, expect, it } from "vitest";
import { useLevelStore } from "../../lib/levels";
import { useSatelliteStore } from "../../lib/satellite";
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

function channel(rejected: number, skipped = 0): ChannelInfo {
  return {
    id: 3,
    stream: 0,
    rejected,
    skipped,
    settings: {
      frequency_hz: 2_426_000_000,
      params: { type: "ble", settings: { link: "channel" } },
    },
  };
}

function face(rejected: number, skipped = 0): string {
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
    channels: [channel(rejected, skipped)],
    overruns: 0,
  };
  return renderFace(ChannelFace, BLE, {
    graph: GRAPH,
    devices: new Map([["radio", set]]),
    channels: new Map([["ble", channel(rejected, skipped)]]),
    owners: new Map([["ble", "radio"]]),
  });
}

describe("ChannelFace", () => {
  it("shows packets that synced but failed their check", () => {
    const html = face(12);
    expect(html).toContain('title="Rejected: packets heard but failed their check"');
    expect(html).toContain(">12</b>");
  });

  it("shows windows the decoder had no time for", () => {
    const html = face(0, 4);
    expect(html).toContain('title="Skipped: windows the decoder had no time for"');
    expect(html).toContain(">4</b>");
  });

  it("says nothing while every packet reads", () => {
    const html = face(0);
    expect(html).not.toContain("Rejected");
    expect(html).not.toContain("Skipped");
  });

  it("marks the face while audio passes the squelch", () => {
    expect(face(0)).not.toContain("Passing audio");
    useLevelStore.setState({
      byDeviceSet: { 1: { 3: { channel: 3, level_db: -40, peak_db: -30, open: true } } },
    });
    expect(face(0)).toContain('aria-label="Passing audio"');
  });

  it("names no satellite that drives it without a wire", () => {
    useSatelliteStore.setState({
      byNode: { sat: { node: "sat", name: "NOAA 19", driving: ["ble"], visible: true } },
    });
    expect(face(0)).not.toContain("NOAA 19");
  });
});

afterEach(() => {
  useSatelliteStore.setState({ byNode: {} });
  useLevelStore.getState().reset();
});
