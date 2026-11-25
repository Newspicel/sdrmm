import { beforeAll, describe, expect, it } from "vitest";
import { loadCatalog } from "../lib/catalog";
import { MAX_CHANNELS_PER_RADIO, MAX_COORD, MAX_EDGES, MAX_NODES } from "../lib/limits";
import type {
  ChannelDescriptor,
  EncoderDescriptor,
  PatchGraph,
  PatchNode,
  PortRef,
  WireVerdict,
} from "../lib/types";
import { CATALOG, channelPortsNamed } from "../test/catalog";
import { declaredPorts } from "./graph";
import {
  footprint,
  sectionOf,
  streamPort,
  TEST_RADIOS,
  type TestContext,
  type TestWorkspace,
  testWorkspaces,
  type Wiring,
} from "./testWorkspace";

function decoder(typeId: string, ...ports: string[]): ChannelDescriptor {
  return {
    type_id: typeId,
    name: typeId,
    band: { low_hz: -6_250, high_hz: 6_250 },
    input_rate_hz: 48_000,
    ports: channelPortsNamed(...ports),
  };
}

const VOICE = decoder("voice", "iq", "control", "baseband", "audio", "events");
const PLANE = decoder("plane", "iq", "control", "position", "events");
const TV = decoder("tv", "iq", "control", "video");

const ENCODERS: EncoderDescriptor[] = [
  { type_id: "fm", name: "FM", bandwidth_hz: 12_500, rate_hz: 48_000, input: "audio" },
];

const CONTEXT: TestContext = {
  catalog: CATALOG,
  channelTypes: [VOICE, PLANE, TV],
  encoderTypes: ENCODERS,
};

const MANY: TestContext = {
  ...CONTEXT,
  channelTypes: Array.from({ length: Math.ceil(MAX_NODES / 3) + 1 }, (_, index) =>
    decoder(`voice${index}`, "iq", "control", "audio", "events"),
  ),
};

/** Stands in for the server: every input of the output's type takes it, unless `refuse` says. */
function stubWiring(
  context: TestContext,
  refuse: (from: PatchNode, to: PatchNode) => boolean = () => false,
): Wiring {
  return async (graph: PatchGraph, from: PortRef): Promise<WireVerdict[]> => {
    const source = graph.nodes.find((node) => node.id === from.node);
    const out = source && declaredPorts(context, source).find((port) => port.name === from.port);
    if (source === undefined || out === undefined) {
      return [];
    }
    return graph.nodes
      .filter((node) => node.id !== source.id)
      .flatMap((node) =>
        declaredPorts(context, node)
          .filter((port) => port.direction === "in" && port.port_type === out.port_type)
          .map((port) => ({
            to: { node: node.id, port: port.name },
            refusal: refuse(source, node) ? "refused" : null,
          })),
      );
  };
}

/** Stands in for the server where a radio holds `MAX_CHANNELS_PER_RADIO` channels. */
function fullRadios(context: TestContext): Wiring {
  const base = stubWiring(context);
  return async (graph: PatchGraph, from: PortRef): Promise<WireVerdict[]> => {
    const kind = new Map(graph.nodes.map((node) => [node.id, node.kind]));
    const held = (graph.edges ?? []).filter(
      (edge) => edge.from.node === from.node && kind.get(edge.to.node) === "channel",
    ).length;
    const full = kind.get(from.node) === "device" && held >= MAX_CHANNELS_PER_RADIO;
    return (await base(graph, from)).map((verdict) =>
      full && kind.get(verdict.to.node) === "channel" ? { ...verdict, refusal: "full" } : verdict,
    );
  };
}

function channelIntoMap(from: PatchNode, to: PatchNode): boolean {
  return from.kind === "channel" && to.kind === "map";
}

function build(context: TestContext, scale = 1): Promise<TestWorkspace[]> {
  return testWorkspaces(context, stubWiring(context), scale);
}

let few: TestWorkspace[] = [];
let many: TestWorkspace[] = [];

beforeAll(async () => {
  loadCatalog(CATALOG);
  few = await build(CONTEXT);
  many = await build(MANY, 1.25);
});

function nodesOf(workspaces: readonly TestWorkspace[]): PatchNode[] {
  return workspaces.flatMap((workspace) => workspace.snapshot.graph.nodes);
}

function typeIdOf(node: PatchNode): string {
  switch (node.kind) {
    case "channel":
      return node.data.channel_type;
    case "encoder":
      return node.data.encoder_type;
    case "processor":
      return node.data?.processor_type ?? "";
    default:
      return "";
  }
}

function subjects(context: TestContext): string[] {
  const picked = [
    ...context.channelTypes.map((type) => `channel:${type.type_id}`),
    ...(context.encoderTypes ?? []).map((type) => `encoder:${type.type_id}`),
    ...(CATALOG.processors ?? []).map((type) => `processor:${type.type_id}`),
  ];
  const plain = CATALOG.nodes
    .filter((entry) => entry.picks == null)
    .map((entry) => `${entry.kind}:`);
  return [...plain, ...picked];
}

describe("testWorkspaces", () => {
  it("draws every kind, decoder, encoder and processor", () => {
    const drawn = new Set(nodesOf(few).map((node) => `${node.kind}:${typeIdOf(node)}`));
    expect(subjects(CONTEXT).filter((subject) => !drawn.has(subject))).toEqual([]);
  });

  it("draws a kind with inputs unwired, stopped and running", () => {
    const nodes = nodesOf(few);
    const sections = (kind: string) =>
      new Set(nodes.filter((node) => node.kind === kind).map(sectionOf));
    expect(sections("scope")).toEqual(new Set(["unwired", "stopped", "running"]));
    expect(sections("gps")).toEqual(new Set(["sources"]));
    expect(sections("device")).toEqual(new Set(["unwired", "stopped", "running"]));
  });

  it("opens only virtual radios, stopped in the stopped rows", () => {
    const radios = nodesOf(few).filter(
      (node) => node.kind === "device" && sectionOf(node) !== "unwired",
    );
    expect(radios.length).toBeGreaterThan(0);
    for (const radio of radios) {
      expect(radio.kind === "device" && radio.data?.device?.backend).toBe("virtual");
      expect(radio.kind === "device" && radio.data?.stopped).toBe(sectionOf(radio) === "stopped");
    }
  });

  it("connects every input of a wired copy", () => {
    for (const { snapshot } of few) {
      const { nodes, edges = [] } = snapshot.graph;
      const wired = nodes.filter(
        (node) =>
          node.kind !== "device" &&
          (sectionOf(node) === "stopped" || sectionOf(node) === "running"),
      );
      for (const node of wired) {
        const inputs = declaredPorts(CONTEXT, node).filter((port) => port.direction === "in");
        const fed = (name: string) =>
          edges.filter((edge) => edge.to.node === node.id && edge.to.port === name).length;
        for (const input of inputs) {
          const name = input.repeat === "per_lane" ? streamPort(input.name, 1) : input.name;
          const group = inputs.filter((port) => input.group != null && port.group === input.group);
          const wires =
            group.length > 0 ? group.reduce((sum, port) => sum + fed(port.name), 0) : fed(name);
          expect(wires, `${node.id}.${name}`).toBe(1);
        }
      }
    }
  });

  it("never wires an unwired copy", () => {
    for (const { snapshot } of few) {
      const unwired = new Set(
        snapshot.graph.nodes.filter((node) => sectionOf(node) === "unwired").map((node) => node.id),
      );
      const touching = (snapshot.graph.edges ?? []).filter(
        (edge) => unwired.has(edge.from.node) || unwired.has(edge.to.node),
      );
      expect(touching).toEqual([]);
    }
  });

  it("feeds a lane input from every stream of the bench array", () => {
    const { nodes, edges = [] } = few[0]?.snapshot.graph ?? { nodes: [] };
    const array = nodes.find((node) => node.id === "running-array");
    const lanes = edges.filter(
      (edge) => edge.to.node === array?.id && edge.to.port.startsWith("lane"),
    );
    expect(lanes.map((edge) => edge.to.port)).toEqual(
      Array.from({ length: TEST_RADIOS.lanes.streams }, (_, index) => streamPort("lane", index)),
    );
  });

  it("gives a processor that needs spread tuning an array of its own on the dongles", () => {
    const graphs = few.map(({ snapshot }) => snapshot.graph);
    for (const state of ["stopped", "running"]) {
      const graph = graphs.find((held) =>
        held.nodes.some((node) => node.id === `${state}-array-spread`),
      );
      const edges = graph?.edges ?? [];
      const array = graph?.nodes.find((node) => node.id === `${state}-array-spread`);
      expect(array?.kind === "array" ? array.data.tuning : null).toBe("spread");
      const readers = edges
        .filter((edge) => edge.from.node === array?.id)
        .map((edge) => edge.to.node);
      expect(readers).toEqual([`${state}-processor-stitch`]);
      const lanes = edges
        .filter((edge) => edge.to.node === array?.id && edge.to.port.startsWith("lane"))
        .map((edge) => edge.from.node);
      expect(lanes).toEqual(TEST_RADIOS.spread.keys.map((key) => `${state}-device-${key}`));
      const together = edges.filter((edge) => edge.from.node === `${state}-array`);
      expect(together.some((edge) => edge.to.node === `${state}-processor-stitch`)).toBe(false);
    }
  });

  it("splits a catalog too big for one patch and keeps each within the limits", () => {
    expect(many.length).toBeGreaterThan(1);
    for (const { snapshot } of many) {
      expect(snapshot.graph.nodes.length).toBeLessThanOrEqual(MAX_NODES);
      expect((snapshot.graph.edges ?? []).length).toBeLessThanOrEqual(MAX_EDGES);
      expect(new Set(snapshot.graph.nodes.map((node) => node.id)).size).toBe(
        snapshot.graph.nodes.length,
      );
    }
    const drawn = new Set(nodesOf(many).map((node) => `${node.kind}:${typeIdOf(node)}`));
    expect(subjects(MANY).filter((subject) => !drawn.has(subject))).toEqual([]);
  });

  it("lays out faces without overlap, inside the canvas bounds", () => {
    for (const { snapshot } of many) {
      const boxes = snapshot.graph.nodes.map((node) => ({
        ...node.position,
        ...footprint(node, 1.25),
      }));
      boxes.forEach((a, index) => {
        expect(a.x + a.w).toBeLessThan(MAX_COORD);
        expect(a.y + a.h).toBeLessThan(MAX_COORD);
        for (const b of boxes.slice(index + 1)) {
          const apart =
            a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
          expect(apart).toBe(true);
        }
      });
    }
  });

  it("puts each node's unwired, stopped and running copies side by side", () => {
    const states = ["unwired", "stopped", "running"];
    for (const { snapshot } of [...few, ...many]) {
      const rows = new Map<string, PatchNode[]>();
      for (const node of snapshot.graph.nodes) {
        const state = states.find((held) => node.id.startsWith(`${held}-`));
        if (state !== undefined && !node.id.includes("-label")) {
          const key = node.id.slice(state.length + 1);
          rows.set(key, [...(rows.get(key) ?? []), node]);
        }
      }
      if (snapshot === few[0]?.snapshot) {
        expect((rows.get("channel-voice") ?? []).map((node) => sectionOf(node))).toEqual(states);
      }
      for (const copies of rows.values()) {
        const ordered = copies.toSorted(
          (a, b) => states.indexOf(sectionOf(a) ?? "") - states.indexOf(sectionOf(b) ?? ""),
        );
        expect(new Set(ordered.map((node) => node.position.y)).size).toBe(1);
        ordered.slice(1).forEach((node, index) => {
          expect(node.position.x).toBeGreaterThan(ordered[index]?.position.x ?? 0);
        });
      }
    }
  });

  it("heads every column with its state", () => {
    const notes = few[0]?.snapshot.graph.nodes.filter((node) => node.kind === "note") ?? [];
    const text = notes.map((node) => (node.kind === "note" ? node.data.text : ""));
    expect(text).toEqual(expect.arrayContaining(["## Unwired", "## Stopped", "## Running"]));
  });

  it("gives a resizable face with ports a larger size", () => {
    const scope = nodesOf(few).find((node) => node.kind === "scope");
    const face = CATALOG.nodes.find((entry) => entry.kind === "scope")?.face;
    expect(scope?.size?.w).toBeGreaterThan(face?.width ?? 0);
  });

  it("draws no wire the server refuses", async () => {
    const workspaces = await testWorkspaces(CONTEXT, stubWiring(CONTEXT, channelIntoMap));
    const kind = new Map(nodesOf(workspaces).map((node) => [node.id, node.kind]));
    const edges = workspaces.flatMap(({ snapshot }) => snapshot.graph.edges ?? []);
    const maps = edges.filter((edge) => kind.get(edge.to.node) === "map");
    expect(maps.filter((edge) => kind.get(edge.from.node) === "channel")).toEqual([]);
    expect(maps.some((edge) => edge.to.port === "events")).toBe(true);
  });

  it("draws another radio when the held ones are full of channels", async () => {
    const context: TestContext = {
      ...CONTEXT,
      channelTypes: Array.from({ length: MAX_CHANNELS_PER_RADIO * 2 + 1 }, (_, index) =>
        decoder(`voice${index}`, "iq", "audio"),
      ),
    };
    const workspaces = await testWorkspaces(context, fullRadios(context));
    for (const { snapshot } of workspaces) {
      const { nodes, edges = [] } = snapshot.graph;
      const kind = new Map(nodes.map((node) => [node.id, node.kind]));
      const fed = edges.filter(
        (edge) => kind.get(edge.from.node) === "device" && kind.get(edge.to.node) === "channel",
      );
      const wired = nodes.filter(
        (node) =>
          node.kind === "channel" &&
          (sectionOf(node) === "running" || sectionOf(node) === "stopped"),
      );
      for (const channel of wired) {
        expect(
          fed.filter((edge) => edge.to.node === channel.id),
          channel.id,
        ).toHaveLength(1);
      }
      for (const radio of new Set(fed.map((edge) => edge.from.node))) {
        const held = fed.filter((edge) => edge.from.node === radio).length;
        expect(held, radio).toBeLessThanOrEqual(MAX_CHANNELS_PER_RADIO);
      }
    }
    const ids = new Set(nodesOf(workspaces).map((node) => node.id));
    for (const state of ["stopped", "running"]) {
      for (const suffix of ["", "-2", "-3"]) {
        expect(ids.has(`${state}-device${suffix}`), state + suffix).toBe(true);
      }
    }
  });
});
