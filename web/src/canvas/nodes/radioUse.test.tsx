import { describe, expect, it } from "vitest";
import type { DeviceSet, Duplex, PatchGraph, PatchNode, RadioUse } from "../../lib/types";
import { CATALOG, catalogBody } from "../../test/catalog";
import { renderFace } from "../../test/faceHarness";
import { capabilities, deviceSet, placed } from "../../test/fixtures";
import { resolvedBy } from "../../test/recorded";
import type { GraphContext } from "../graph";
import { DeviceFace } from "./DeviceFace";
import { radioUseOf, withRadioUse } from "./radioUse";

function radio(radioUse?: RadioUse): PatchNode {
  return placed("radio", {
    kind: "device",
    data: { device: { backend: "virtual", key: "halfduplex" }, radio_use: radioUse },
  });
}

function open(duplex: Duplex, uses: RadioUse[], radioUse: RadioUse = "receive"): DeviceSet {
  return deviceSet({
    device: { driver: "virtual", key: "halfduplex", label: "Half-duplex 1×1 (virtual)" },
    capabilities: capabilities({ duplex }),
    radio_use: radioUse,
    uses,
  });
}

describe("radioUseOf", () => {
  it("reads the node's setting, receive when it names none", () => {
    expect(radioUseOf(radio(), null)).toBe("receive");
    expect(radioUseOf(radio("both"), null)).toBe("both");
  });

  it("follows the radio when it cannot do what the node asks", () => {
    expect(radioUseOf(radio(), open("tx_only", ["send"], "send"))).toBe("send");
    expect(radioUseOf(radio("send"), open("half", ["receive", "send", "both"]))).toBe("send");
  });
});

function inspected(set: DeviceSet): string {
  const node = radio();
  return renderFace(DeviceFace, node, {
    graph: { nodes: [node], edges: [] },
    devices: new Map([["radio", set]]),
    inspected: true,
  });
}

describe("the Use setting", () => {
  it("is a dropdown on a radio that can send", () => {
    const html = inspected(open("half", ["receive", "send", "both"]));
    expect(html).toContain(">Use<");
    expect(html).toContain("Receive");
  });

  it("is absent on a receiver", () => {
    expect(inspected(open("rx_only", ["receive"]))).not.toContain(">Use<");
  });
});

function both(): PatchGraph {
  return {
    nodes: [
      radio("both"),
      placed("scope", { kind: "scope" }),
      placed("tx", catalogBody("transmit")),
    ],
    edges: [
      { from: { node: "radio", port: "iq" }, to: { node: "scope", port: "iq" } },
      { from: { node: "tx", port: "transmit" }, to: { node: "radio", port: "transmit" } },
    ],
  };
}

function wiresAfter(radioUse: RadioUse): string[] {
  const graph = both();
  const context: GraphContext = { catalog: CATALOG, channelTypes: [], resolved: resolvedBy(graph) };
  const used = withRadioUse(graph, context, "radio", radioUse);
  const node = used.nodes.find((found) => found.id === "radio");
  expect(node?.kind === "device" ? node.data.radio_use : null).toBe(radioUse);
  return (used.edges ?? []).map((edge) => `${edge.from.node}.${edge.from.port}`);
}

describe("withRadioUse", () => {
  it("drops the wires on ports the new use leaves out, in the same edit", () => {
    expect(wiresAfter("send")).toEqual(["tx.transmit"]);
    expect(wiresAfter("receive")).toEqual(["radio.iq"]);
    expect(wiresAfter("both")).toEqual(["radio.iq", "tx.transmit"]);
  });
});
