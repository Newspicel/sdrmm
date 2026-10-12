import { describe, expect, it } from "vitest";
import type { DeviceInfo, PatchGraph, PatchNode } from "../../lib/types";
import { resolvedNodeOf, resolvedPatch, resolvedPort } from "../../test/resolved";
import {
  channelBinding,
  channelBindingAction,
  channelBindingHint,
  channelBindingStatus,
  lockedChannels,
  radioIsAttached,
  radioRefsOf,
} from "./channelNode";

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

const iqFrom = (source: string) =>
  resolvedPort(
    { name: "iq", port_type: "iq", direction: "in" },
    { feeds: [{ source, port: "iq" }] },
  );

describe("radioRefsOf", () => {
  it("names the radios the resolved iq feeds come from", () => {
    const resolved = resolvedPatch([
      resolvedNodeOf("dev", "device", [], { runs: "unbound", reason: "Radio not open" }),
      resolvedNodeOf("blank", "device"),
      resolvedNodeOf("nfm", "channel", [iqFrom("dev")]),
      resolvedNodeOf("empty", "channel", [iqFrom("blank")]),
      resolvedNodeOf("loose", "channel", []),
    ]);
    expect(radioRefsOf(graph, resolved, "nfm")).toEqual([{ backend: "rtlsdr", serial: "A" }]);
    expect(radioRefsOf(graph, resolved, "empty")).toEqual([]);
    expect(radioRefsOf(graph, resolved, "loose")).toEqual([]);
    expect(radioRefsOf(graph, null, "nfm")).toEqual([]);
  });
});

describe("radioIsAttached", () => {
  const attached: DeviceInfo[] = [{ driver: "rtlsdr", key: "0", label: "RTL-SDR", serial: "A" }];

  it("only matches a named radio that is on the bus", () => {
    expect(radioIsAttached([{ backend: "rtlsdr", serial: "A" }], attached)).toBe(true);
    expect(radioIsAttached([{ backend: "rtlsdr", serial: "B" }], attached)).toBe(false);
    expect(radioIsAttached([{ backend: "rtlsdr", serial: "A" }], [])).toBe(false);
    expect(radioIsAttached([], attached)).toBe(false);
    expect(
      radioIsAttached(
        [
          { backend: "rtlsdr", serial: "B" },
          { backend: "rtlsdr", serial: "A" },
        ],
        attached,
      ),
    ).toBe(true);
  });
});

describe("channelBinding", () => {
  const state = { wired: true, open: false, named: true, attached: true };

  it("reads the wire before the radio", () => {
    expect(channelBinding({ ...state, wired: false, open: true })).toBe("unwired");
    expect(channelBinding({ ...state, open: true })).toBe("not-started");
    expect(channelBinding({ ...state, named: false })).toBe("no-radio");
    expect(channelBinding(state)).toBe("radio-closed");
    expect(channelBinding({ ...state, attached: false })).toBe("radio-absent");
  });

  it("offers an action only where one would do something", () => {
    expect(channelBindingAction("radio-closed")).toBe("Open radio");
    expect(channelBindingAction("not-started")).toBe("Start channel");
    expect(channelBindingAction("radio-absent")).toBeNull();
    expect(channelBindingAction("no-radio")).toBeNull();
    expect(channelBindingAction("unwired")).toBeNull();
  });

  it("hints at each state without putting prose on the face", () => {
    expect(channelBindingHint("radio-absent")).toBe("Its radio is not connected");
    expect(channelBindingHint("unwired")).toBe("Wire a device's IQ in");
  });

  it("names each state in a word or two for the header", () => {
    expect(channelBindingStatus("unwired")).toBe("unwired");
    expect(channelBindingStatus("no-radio")).toBe("no radio");
    expect(channelBindingStatus("radio-absent")).toBe("radio missing");
    expect(channelBindingStatus("radio-closed")).toBe("radio closed");
    expect(channelBindingStatus("not-started")).toBe("not started");
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
