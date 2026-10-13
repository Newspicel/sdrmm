import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  ChannelDescriptor,
  NodeBodyOf,
  NodeKind,
  PatchCatalog,
  PatchGraph,
  PatchNode,
  PortSpec,
} from "../lib/types";
import { catalogBody, channelPortsNamed, CATALOG as REAL_CATALOG } from "../test/catalog";
import { resolvedNodeOf, resolvedPatch, resolvedPort } from "../test/resolved";
import {
  addEdge,
  addNode,
  clampCells,
  echoesWire,
  edgeKey,
  FIT_MIN_W,
  fitWidth,
  type GraphContext,
  handleSignature,
  isPinned,
  isResizable,
  moveSlot,
  newNodeId,
  nodeIds,
  nodeMinSize,
  nodePorts,
  nodeSize,
  PORT_STEP_PX,
  PORT_TOP_PX,
  pin,
  placeSlot,
  portLabel,
  portStream,
  pruneRack,
  RACK_COLS,
  removeNode,
  resizeSlot,
  sameGraph,
  slotRoom,
  streamLabel,
  streamPort,
  unpin,
} from "./graph";

const CATALOG: PatchCatalog = {
  ...REAL_CATALOG,
  nodes: REAL_CATALOG.nodes.filter((entry) =>
    ["device", "channel", "scope", "speaker", "scanner"].includes(entry.kind),
  ),
};

const TYPES: ChannelDescriptor[] = [
  {
    type_id: "nfm",
    name: "NFM",
    bandwidth_hz: 12_500,
    input_rate_hz: 48_000,
    has_audio: true,
    emits: ["tone", "scrambler"],
    calls: "squelch",
    ports: channelPortsNamed("iq", "control", "baseband", "audio", "events"),
  },
  {
    type_id: "adsb",
    name: "ADS-B",
    bandwidth_hz: 2_000_000,
    input_rate_hz: 2_400_000,
    has_audio: false,
    emits: ["adsb"],
    needs_position: true,
    ports: channelPortsNamed("iq", "control", "position", "baseband", "events"),
  },
  {
    type_id: "atv",
    name: "ATV",
    bandwidth_hz: 6_000_000,
    input_rate_hz: 16_000_000,
    has_audio: false,
    has_video: true,
    ports: channelPortsNamed("iq", "control", "baseband", "video"),
  },
];

const context: GraphContext = { catalog: CATALOG, channelTypes: TYPES };

function node(id: string, body: Partial<PatchNode> & Pick<PatchNode, "kind">): PatchNode {
  return { id, position: { x: 0, y: 0 }, ...body } as PatchNode;
}

function workspace(): PatchGraph {
  return {
    nodes: [
      node("dev", { kind: "device", data: {} }),
      node("scope", { kind: "scope" }),
      node("nfm", { kind: "channel", data: { channel_type: "nfm" } }),
      node("spk", { kind: "speaker" }),
    ],
    edges: [
      { from: { node: "dev", port: "iq" }, to: { node: "scope", port: "iq" } },
      { from: { node: "dev", port: "iq" }, to: { node: "nfm", port: "iq" } },
    ],
  };
}

function radioResolved(outputs: readonly string[], inputs: readonly PortSpec[] = []) {
  const ports = [
    ...inputs.map((spec) => resolvedPort(spec)),
    ...outputs.map((name) => resolvedPort(iqOut(name))),
  ];
  return resolvedPatch([resolvedNodeOf("dev", "device", ports, { runs: "radio", device_set: 1 })]);
}

const deviceNode = (graph: PatchGraph): PatchNode => {
  const dev = graph.nodes[0];
  if (dev === undefined) {
    throw new Error("the workspace has a device node");
  }
  return dev;
};

const positionPort = (name: string, direction: "in" | "out"): PortSpec => ({
  name,
  port_type: "position",
  direction,
  multi: "one",
  payload: { type: "position" },
});

function iqOut(name: string): PortSpec {
  return {
    name,
    port_type: "iq",
    direction: "out",
    multi: "many",
    payload: { type: "iq", origins: ["radio"] },
  };
}

describe("echoesWire", () => {
  const graph = {
    nodes: [],
    edges: [
      { from: { node: "gps", port: "position" }, to: { node: "sat", port: "position" } },
      { from: { node: "dev", port: "iq" }, to: { node: "sat", port: "control" } },
    ],
  } as unknown as PatchGraph;

  it("quiets an input wired from an output of the same name", () => {
    expect(echoesWire(graph, "sat", positionPort("position", "in"))).toBe(true);
  });

  it("keeps labels that add something", () => {
    expect(echoesWire(graph, "sat", positionPort("control", "in"))).toBe(false);
    expect(echoesWire(graph, "gps", positionPort("position", "out"))).toBe(false);
    expect(echoesWire(graph, "map", positionPort("position", "in"))).toBe(false);
  });
});

describe("fitWidth", () => {
  it("holds the satellite face at one width", () => {
    const fit = fitWidth("satellite");
    expect(fit?.minWidth).toBe(fit?.maxWidth);
  });
});

describe("portLabel", () => {
  it("numbers the first stream only when there is a second to tell it from", () => {
    const one = [iqOut("iq")];
    expect(portLabel("iq", one)).toBe("iq");

    const two = [iqOut("iq"), iqOut("iq2")];
    expect(portLabel("iq", two)).toBe("iq1");
    expect(portLabel("iq2", two)).toBe("iq2");

    expect(portLabel("control", two)).toBe("control");
  });

  it("leaves the wire name alone", () => {
    expect(streamPort("iq", 0)).toBe("iq");
    expect(streamLabel("iq", 0, 1)).toBe("iq");
    expect(streamLabel("iq", 0, 4)).toBe("iq1");
    expect(streamLabel("iq", 3, 4)).toBe("iq4");
  });
});

describe("handleSignature", () => {
  it("changes when a node gains a port", () => {
    const one = handleSignature([iqOut("iq")]);
    const two = handleSignature([iqOut("iq"), iqOut("iq2")]);
    expect(two).not.toBe(one);
    expect(two).toContain("out:iq2");
  });
});

describe("ports", () => {
  it("draws an unresolved channel with the ports the server serves for its type", () => {
    const graph = workspace();
    const nfm = graph.nodes[2];
    const adsb = node("adsb", { kind: "channel", data: { channel_type: "adsb" } });
    const atv = node("atv", { kind: "channel", data: { channel_type: "atv" } });
    expect(nfm && nodePorts(context, nfm).map((p) => p.name)).toEqual([
      "iq",
      "control",
      "baseband",
      "audio",
      "events",
    ]);
    expect(nodePorts(context, adsb).map((p) => p.name)).toEqual([
      "iq",
      "control",
      "position",
      "baseband",
      "events",
    ]);
    expect(nodePorts(context, atv).map((p) => p.name)).toEqual([
      "iq",
      "control",
      "baseband",
      "video",
    ]);
  });

  it("gives an unknown channel type no ports until the server resolves it", () => {
    const ghost = node("x", { kind: "channel", data: { channel_type: "wefax" } });
    expect(nodePorts(context, ghost)).toEqual([]);
  });

  it("draws the ports the server resolved, and catalog ports until it has", () => {
    const graph = workspace();
    const dev = deviceNode(graph);
    expect(nodePorts(context, dev).map((p) => p.name)).toEqual(["iq"]);
    const four = { ...context, resolved: radioResolved(["iq", "iq2", "iq3", "iq4"]) };
    const ports = nodePorts(four, dev);
    expect(ports.map((p) => p.name)).toEqual(["iq", "iq2", "iq3", "iq4"]);
    expect(ports.every((p) => !("feeds" in p) && !("reaches" in p))).toBe(true);
  });

  it("falls back to catalog ports when the resolved node has another kind", () => {
    const dev = deviceNode(workspace());
    const stale = resolvedPatch([resolvedNodeOf("dev", "scope", [])]);
    expect(nodePorts({ ...context, resolved: stale }, dev).map((p) => p.name)).toEqual(["iq"]);
  });

  it("numbers stream ports from two and answers only canonical spellings", () => {
    expect(streamPort("iq", 0)).toBe("iq");
    expect(streamPort("iq", 2)).toBe("iq3");
    expect(portStream("iq", "iq")).toBe(0);
    expect(portStream("iq", "iq16")).toBe(15);
    for (const name of ["iq1", "iq0", "iq02", "iq17", "iqx", "tx2"]) {
      expect(portStream("iq", name)).toBeNull();
    }
  });

  it("grows the resize floor with the port count", () => {
    const deep = Array.from({ length: 20 }, (_, index) => ({
      name: `events${index}`,
      port_type: "events",
      direction: "in",
    })) as PortSpec[];
    const floor = nodeMinSize("decoder_log", []);
    expect(nodeMinSize("decoder_log", deep)).toEqual({
      w: floor.w,
      h: PORT_TOP_PX + PORT_STEP_PX * 20,
    });
    expect(nodeMinSize("decoder_log", deep).h).toBeGreaterThan(floor.h);
    expect(nodeMinSize("decoder_log", deep.slice(0, 1))).toEqual(floor);
  });

  it("resizes the viewports and nothing else", () => {
    const viewports: NodeKind[] = ["scope", "map", "signal_map", "readout", "decoder_log", "video"];
    const controls: NodeKind[] = [
      "device",
      "channel",
      "gps",
      "scanner",
      "recorder",
      "audio_recorder",
      "network_export",
      "event_output",
      "export",
    ];
    expect(viewports.filter((kind) => !isResizable(kind))).toEqual([]);
    expect(controls.filter(isResizable)).toEqual([]);
  });

  it("gives every kind a width and only the viewports a height", () => {
    for (const entry of REAL_CATALOG.nodes) {
      const size = nodeSize(entry.kind);
      expect(size.w).toBe(entry.face.width);
      expect(size.h !== undefined).toBe(isResizable(entry.kind));
    }
  });

  it("floors a content-sized kind at its width and its ports' depth", () => {
    expect(nodeMinSize("speaker", [])).toEqual({ w: nodeSize("speaker").w, h: PORT_TOP_PX });
  });

  it("widens a dial face to the dial floor", () => {
    expect(fitWidth("device")).toEqual({ minWidth: 420, maxWidth: 420 });
    expect(fitWidth("speaker")).toEqual({ minWidth: FIT_MIN_W, maxWidth: 320 });
    expect(fitWidth("map")).toBeNull();
  });
});

describe("node ids", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("names a node after its kind and keeps every one distinct", () => {
    const taken = nodeIds(workspace());
    const first = newNodeId("scope", taken);
    taken.add(first);

    expect(first).toMatch(/^scope:[0-9a-f]{8}$/);
    expect(newNodeId("scope", taken)).not.toEqual(first);
  });

  it("mints them where randomUUID is absent, as on a plain-HTTP origin", () => {
    const real = globalThis.crypto;
    vi.stubGlobal("crypto", {
      getRandomValues: (buffer: Uint8Array<ArrayBuffer>) => real.getRandomValues(buffer),
    });

    expect(newNodeId("scope", new Set())).toMatch(/^scope:[0-9a-f]{8}$/);
  });

  it("draws again when the id it drew is already spoken for", () => {
    const drawn = [0x11, 0x22];
    vi.stubGlobal("crypto", {
      getRandomValues: (buffer: Uint8Array) => {
        buffer.fill(drawn.shift() ?? 0x33);
        return buffer;
      },
    });

    expect(newNodeId("scope", new Set(["scope:11111111"]))).toBe("scope:22222222");
  });
});

describe("editing", () => {
  it("removing a node takes its wires with it", () => {
    const graph = removeNode(workspace(), "nfm");
    expect(graph.nodes.map((n) => n.id)).toEqual(["dev", "scope", "spk"]);
    expect(graph.edges?.map(edgeKey)).toEqual(["dev.iq->scope.iq"]);
  });

  it("refuses to draw two nodes under one id", () => {
    const graph = workspace();
    const drawn = graph.nodes[0];
    if (drawn === undefined) {
      throw new Error("an empty patch");
    }
    expect(() => addNode(graph, drawn)).toThrow(/duplicate node id/);
  });

  it("compares graphs structurally so an echo of our own write is not re-applied", () => {
    expect(sameGraph(workspace(), workspace())).toBe(true);
    expect(
      sameGraph(
        workspace(),
        addEdge(workspace(), {
          from: { node: "nfm", port: "audio" },
          to: { node: "spk", port: "audio" },
        }),
      ),
    ).toBe(false);
  });
});

describe("the rack", () => {
  it("pins into the first free cell and unpins", () => {
    let rack = pin({ slots: [] }, "scope");
    expect(rack.slots).toEqual([{ node: "scope", x: 0, y: 0, w: 6, h: 4 }]);
    rack = pin(rack, "nfm");
    expect(rack.slots?.[1]).toEqual({ node: "nfm", x: 6, y: 0, w: 6, h: 4 });
    rack = pin(rack, "spk");
    expect(rack.slots?.[2]).toEqual({ node: "spk", x: 0, y: 4, w: 6, h: 4 });

    expect(isPinned(rack, "nfm")).toBe(true);
    rack = unpin(rack, "nfm");
    expect(isPinned(rack, "nfm")).toBe(false);
    expect(pin(rack, "scope")).toBe(rack);
  });

  it("refuses a placement that overlaps or leaves the grid", () => {
    const rack = pin(pin({ slots: [] }, "a"), "b");
    expect(placeSlot(rack, "b", { x: 0, y: 0, w: 6, h: 4 })).toBe(rack);
    expect(placeSlot(rack, "b", { x: RACK_COLS - 2, y: 0, w: 6, h: 4 })).toBe(rack);
    expect(placeSlot(rack, "b", { x: 0, y: 4, w: 6, h: 4 }).slots?.[1]).toEqual({
      node: "b",
      x: 0,
      y: 4,
      w: 6,
      h: 4,
    });
    expect(placeSlot(rack, "a", { x: 0, y: 0, w: 6, h: 8 }).slots?.[0]?.h).toBe(8);
  });

  it("trades places when a face is dropped on another", () => {
    const rack = placeSlot(pin(pin({ slots: [] }, "a"), "b"), "b", { x: 6, y: 0, w: 6, h: 8 });
    const swapped = moveSlot(rack, "a", { x: 6, y: 0 });
    expect(swapped.slots).toEqual([
      { node: "a", x: 6, y: 0, w: 6, h: 8 },
      { node: "b", x: 0, y: 0, w: 6, h: 4 },
    ]);
    expect(moveSlot(rack, "a", { x: 0, y: 4 }).slots?.[0]).toEqual({
      node: "a",
      x: 0,
      y: 4,
      w: 6,
      h: 4,
    });
    expect(moveSlot(rack, "a", { x: RACK_COLS - 1, y: 0 })).toBe(rack);
    const three = placeSlot(pin(rack, "c"), "c", { x: 6, y: 4, w: 6, h: 4 });
    expect(moveSlot(three, "a", { x: 5, y: 2 })).toBe(three);
  });

  it("moves the boundary between two faces, one growing as the other shrinks", () => {
    const rack = pin(pin({ slots: [] }, "a"), "b");
    const wider = resizeSlot(rack, "a", "e", 2);
    expect(wider.slots).toEqual([
      { node: "a", x: 0, y: 0, w: 8, h: 4 },
      { node: "b", x: 8, y: 0, w: 4, h: 4 },
    ]);
    expect(resizeSlot(wider, "b", "w", -2)).toEqual(rack);
    expect(resizeSlot(rack, "a", "e", 6)).toBe(rack);
    expect(resizeSlot(rack, "a", "s", 2).slots?.[0]?.h).toBe(6);
    expect(resizeSlot(rack, "a", "s", 6)).toBe(rack);
    const stacked = placeSlot(pin(rack, "c"), "c", { x: 6, y: 4, w: 6, h: 4 });
    expect(resizeSlot(stacked, "a", "s", 1).slots).toEqual([
      { node: "a", x: 0, y: 0, w: 6, h: 5 },
      { node: "b", x: 6, y: 0, w: 6, h: 4 },
      { node: "c", x: 6, y: 4, w: 6, h: 4 },
    ]);
  });

  it("says how far an edge can travel before the grid or a neighbour stops it", () => {
    const rack = pin(pin({ slots: [] }, "a"), "b");

    expect(slotRoom(rack, "a", "e")).toEqual({ min: -5, max: 5 });
    expect(slotRoom(rack, "a", "s")).toEqual({ min: -3, max: 4 });
    expect(slotRoom(rack, "a", "n")).toEqual({ min: 0, max: 3 });
    expect(slotRoom(rack, "a", "w")).toEqual({ min: 0, max: 5 });
    expect(slotRoom(rack, "gone", "e")).toEqual({ min: 0, max: 0 });

    const stacked = placeSlot(pin(rack, "c"), "c", { x: 0, y: 4, w: 6, h: 4 });
    expect(slotRoom(stacked, "a", "s")).toEqual({ min: -3, max: 3 });

    expect(clampCells(slotRoom(rack, "a", "e"), 9)).toBe(5);
    expect(clampCells(slotRoom(rack, "a", "e"), -9)).toBe(-5);
    expect(clampCells(slotRoom(rack, "a", "e"), 2)).toBe(2);
  });

  it("drops slots whose node is gone and re-places ones the grid no longer holds", () => {
    const rack = pin({ slots: [] }, "nfm");
    expect(pruneRack(rack, removeNode(workspace(), "nfm")).slots).toEqual([]);
    expect(pruneRack(rack, workspace())).toBe(rack);

    const stale = { slots: [{ node: "nfm", x: 12, y: 12, w: 12, h: 8 }] };
    expect(pruneRack(stale, workspace()).slots).toEqual([{ node: "nfm", x: 0, y: 0, w: 6, h: 4 }]);
  });
});

const ARRAY_CONTEXT: GraphContext = { catalog: REAL_CATALOG, channelTypes: [] };

function arrayNodeOf(geometry?: object): PatchNode {
  const body = catalogBody("array") as NodeBodyOf<"array">;
  const data = geometry === undefined ? body.data : { ...body.data, geometry };
  return { id: "arr", position: { x: 0, y: 0 }, kind: "array", data } as PatchNode;
}

function placedNode(id: string, body: object): PatchNode {
  return { id, position: { x: 0, y: 0 }, ...body } as PatchNode;
}

function lanesWired(lanes: readonly number[], geometry?: object): PatchGraph {
  return {
    nodes: [placedNode("dev", { kind: "device", data: {} }), arrayNodeOf(geometry)],
    edges: lanes.map((lane) => ({
      from: { node: "dev", port: streamPort("iq", lane) },
      to: { node: "arr", port: streamPort("lane", lane) },
    })),
  };
}

function laneInputs(graph: PatchGraph, lanes: number): string[] {
  const array = graph.nodes.find((candidate) => candidate.id === "arr");
  if (array === undefined) {
    return [];
  }
  const resolved = resolvedPatch([
    resolvedNodeOf(
      "arr",
      "array",
      Array.from({ length: lanes }, (_, lane) =>
        resolvedPort({ name: streamPort("lane", lane), port_type: "iq", direction: "in" }),
      ),
      { runs: "engine" },
    ),
  ]);
  const ports = nodePorts({ ...ARRAY_CONTEXT, resolved }, array);
  return ports
    .filter((spec) => spec.port_type === "iq" && spec.direction === "in")
    .map((spec) => portLabel(spec.name, ports));
}

describe("arrays", () => {
  it("labels the resolved lane inputs from one", () => {
    expect(laneInputs(lanesWired([]), 1)).toEqual(["lane"]);
    expect(laneInputs(lanesWired([0, 1]), 3)).toEqual(["lane1", "lane2", "lane3"]);
  });

  it("sizes new processor kinds", () => {
    expect(nodeSize("array")).toEqual({ w: 460 });
    expect(nodeSize("passive_radar")).toEqual({ w: 560, h: 540 });
    expect(isResizable("spatial_spectrum")).toBe(true);
    expect(isResizable("polarimeter")).toBe(false);
    expect(nodeMinSize("passive_radar", []).w).toBe(420);
    expect(nodeMinSize("spatial_spectrum", [])).toEqual({ w: 400, h: 300 });
    expect(nodeMinSize("correlator", [])).toEqual({ w: 380, h: 340 });
  });
});
