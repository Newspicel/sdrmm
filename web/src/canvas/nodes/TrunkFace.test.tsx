import { describe, expect, it } from "vitest";
import type { ChannelDescriptor, PatchNode, TrunkSystemStatus } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { TrunkFace } from "./TrunkFace";

const DMR: ChannelDescriptor = {
  type_id: "dmr",
  name: "DMR",
  band: { low_hz: -6_250, high_hz: 6_250 },
  input_rate_hz: 48_000,
  trunking: {
    slots: 2,
    schemes: [{ id: "tier_three", label: "Tier III", kind: "grants" }],
  },
};

function trunk(data: Partial<Extract<PatchNode, { kind: "trunk" }>["data"]> = {}): PatchNode {
  return placed("trunk", { kind: "trunk", data: { control_hz: 451_000_000, ...data } });
}

function render(node: PatchNode, status?: TrunkSystemStatus, inspected = false): string {
  return renderFace(TrunkFace, node, {
    inspected,
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
      source: 5678,
      group_call: true,
      active: true,
    },
  ],
  problems: [],
  site: { network_id: 12, site_id: 3 },
};

describe("TrunkFace", () => {
  it("offers the protocols the decoder declares", () => {
    const node = trunk({ protocol: "tier_three" });
    expect(render(node, undefined, true)).toContain("Tier III");
    expect(render(node)).not.toContain("Tier III");
  });

  it("shows the talkgroup each traffic channel was granted to", () => {
    const markup = render(trunk(), STATUS);
    expect(markup).toContain("TS 2");
    expect(markup).toContain("TG 91");
  });

  it("shows a named talkgroup by its name", () => {
    const named = trunk({
      talkgroups: {
        priority: [91],
        lockout: [],
        listed_only: false,
        names: [{ id: 91, name: "Fire Dispatch" }],
      },
    });
    expect(render(named, STATUS)).toContain("Fire Dispatch ← 5678");
    expect(render(named, undefined, true)).toContain("Fire Dispatch");
  });

  it("tells an idle receiver from a live call", () => {
    const live = render(trunk(), STATUS);
    const idle = render(trunk(), {
      ...STATUS,
      followers: STATUS.followers.map((follower) => ({ ...follower, active: false })),
    });
    expect(live).toContain("text-ok");
    expect(idle).toContain("text-ink-faint");
  });

  it("keeps its readouts before anything is heard", () => {
    const markup = render(trunk());
    expect(markup).toContain("Site");
    expect(markup).toContain("Call");
  });

  it("names the site and the control channel it moved to", () => {
    const markup = render(trunk(), STATUS);
    expect(markup).toContain("net 12 · site 3");
    expect(markup).toMatch(/Control<\/dt><dd[^>]*>451\.025 /);
  });

  it("lists each talkgroup with how it is followed", () => {
    const markup = render(
      trunk({
        talkgroups: { priority: [101, 102], lockout: [900], listed_only: false, names: [] },
      }),
      undefined,
      true,
    );
    expect(markup).toContain('value="101"');
    expect(markup).toContain('value="900"');
    expect(markup).toContain("Locked out");
    expect(markup).toContain("Priority only");
  });

  it("says when the follower fell behind", () => {
    expect(render(trunk(), { ...STATUS, dropped: 4 })).toContain("4 frames dropped");
  });
});
