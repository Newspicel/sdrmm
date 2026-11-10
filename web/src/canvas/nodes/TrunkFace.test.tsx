import { describe, expect, it } from "vitest";
import type { ChannelDescriptor, PatchNode, TrunkSystemStatus } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { TrunkFace } from "./TrunkFace";

const DMR: ChannelDescriptor = {
  type_id: "dmr",
  name: "DMR",
  bandwidth_hz: 12_500,
  input_rate_hz: 48_000,
  trunking: {
    slots: 2,
    schemes: [{ id: "tier_three", label: "Tier III", kind: "grants" }],
  },
};

function trunk(data: Partial<Extract<PatchNode, { kind: "trunk" }>["data"]> = {}): PatchNode {
  return placed("trunk", { kind: "trunk", data: { control_hz: 451_000_000, ...data } });
}

function render(node: PatchNode, status?: TrunkSystemStatus): string {
  return renderFace(TrunkFace, node, {
    graph: { nodes: [node], edges: [] },
    resolved: null,
    channelTypes: [DMR],
    trunks: status === undefined ? [] : [status],
  });
}

const STATUS: TrunkSystemStatus = {
  node: "trunk",
  detected: "tier_three",
  carriers: 1,
  control: { device_set: 1, channel: 1, freq_hz: 451_025_000 },
  followers: [
    {
      device_set: 1,
      channel: 2,
      slot: 2,
      freq_hz: 451_012_500,
      logical_channel: 17,
      destination: 91,
      group_call: true,
    },
  ],
  problems: [],
  site: { network_id: 12, site_id: 3 },
};

describe("TrunkFace", () => {
  it("offers the protocols the decoder declares", () => {
    expect(render(trunk({ protocol: "tier_three" }))).toContain("Tier III");
  });

  it("shows the talkgroup each traffic channel was granted to", () => {
    const markup = render(trunk(), STATUS);
    expect(markup).toContain("TS 2");
    expect(markup).toContain("TG 91");
  });

  it("names the site and the control channel it moved to", () => {
    const markup = render(trunk(), STATUS);
    expect(markup).toContain("net 12 · site 3");
    expect(markup).toMatch(/Control<\/dt><dd[^>]*>451\.025 /);
  });

  it("lists the priority and locked out talkgroups", () => {
    const markup = render(
      trunk({ talkgroups: { priority: [101, 102], lockout: [900], listed_only: false } }),
    );
    expect(markup).toContain("101, 102");
    expect(markup).toContain("900");
    expect(markup).toContain("Only these");
  });

  it("says when the follower fell behind", () => {
    expect(render(trunk(), { ...STATUS, dropped: 4 })).toContain("4 frames dropped");
  });
});
