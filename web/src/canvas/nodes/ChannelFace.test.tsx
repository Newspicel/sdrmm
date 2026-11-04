import { afterEach, describe, expect, it } from "vitest";
import { paramsWith } from "../../components/channelSettings";
import { useChannelStatusStore } from "../../lib/channelStatus";
import { useLevelStore } from "../../lib/levels";
import { useSatelliteStore } from "../../lib/satellite";
import type { ChannelDescriptor, ChannelInfo, DeviceSet, PatchGraph } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { ChannelFace } from "./ChannelFace";

const RADIO = placed("radio", {
  kind: "device",
  data: { device: { backend: "virtual", key: "band" } },
});
/// The graph resolves against the real registry, so the channel runs a registered decoder; the
/// descriptor beside it is this test's own.
const PROBE = placed("ble", { kind: "channel", data: { channel_type: "ble" } });

const DESCRIPTOR: ChannelDescriptor = {
  type_id: "ble",
  name: "Probe",
  bandwidth_hz: 2_000_000,
  input_rate_hz: 4_000_000,
  has_audio: false,
  settings: [
    {
      key: "link",
      label: "Link",
      control: {
        type: "choice",
        options: [
          { value: "channel", label: "One channel" },
          { value: "band", label: "Whole band" },
        ],
      },
    },
  ],
};

const GRAPH: PatchGraph = {
  nodes: [RADIO, PROBE],
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
      params: paramsWith("ble", { link: "channel" }),
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
  return renderFace(ChannelFace, PROBE, {
    graph: GRAPH,
    devices: new Map([["radio", set]]),
    channels: new Map([["ble", channel(rejected, skipped)]]),
    owners: new Map([["ble", "radio"]]),
    channelTypes: [DESCRIPTOR],
  });
}

describe("ChannelFace", () => {
  it("draws the decoder's settings from its descriptor", () => {
    const html = face(0);
    expect(html).toContain("Link");
    expect(html).toContain("One channel");
  });

  it("shows the readouts the decoder publishes in its status", () => {
    useChannelStatusStore.setState({
      byNode: {
        ble: {
          readouts: [
            {
              label: "SNR",
              value: { type: "number", value: { value: 18.4, unit: "db", digits: 1 } },
            },
          ],
        },
      },
    });
    const html = face(0);
    expect(html).toContain("SNR");
    expect(html).toContain("18.4 dB");
  });

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
  useChannelStatusStore.getState().reset();
  useLevelStore.getState().reset();
});
