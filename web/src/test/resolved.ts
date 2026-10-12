import { portStream } from "../canvas/graph";
import type {
  ChannelInfo,
  DeviceSet,
  Feed,
  NodeKind,
  PatchGraph,
  Payload,
  PortDirection,
  PortSpec,
  PortType,
  Reach,
  ResolvedNode,
  ResolvedPatch,
  ResolvedPort,
  Runs,
  Tap,
} from "../lib/types";
import { CATALOG } from "./catalog";

export function payloadOf(portType: PortType): Payload {
  switch (portType) {
    case "iq":
      return { type: "iq", origins: ["radio"] };
    case "audio":
      return { type: "audio", streams: ["live"] };
    case "events":
      return { type: "events", kinds: [] };
    case "control":
      return { type: "control", verbs: [] };
    default:
      return { type: portType };
  }
}

export type FeedFixture = Omit<Feed, "payload"> & { payload?: Payload };

export function resolvedPort(
  spec: Partial<PortSpec> & Pick<PortSpec, "name" | "port_type" | "direction">,
  links: { feeds?: FeedFixture[]; reaches?: Reach[] } = {},
): ResolvedPort {
  const feeds = links.feeds?.map((feed) => ({
    ...feed,
    payload: feed.payload ?? payloadOf(spec.port_type),
  }));
  return {
    multi: "one",
    payload: payloadOf(spec.port_type),
    ...spec,
    ...(links.reaches === undefined ? {} : { reaches: links.reaches }),
    ...(feeds === undefined ? {} : { feeds }),
  };
}

export function resolvedNodeOf(
  id: string,
  kind: NodeKind,
  ports: ResolvedPort[] = [],
  runs: Runs = { runs: "server" },
): ResolvedNode {
  return { id, kind, label: id, ports, runs };
}

export function resolvedPatch(nodes: ResolvedNode[], workspace = 1): ResolvedPatch {
  return { workspace, revision: 1, epoch: 1, nodes, edges: [] };
}

function specFor(kind: NodeKind, name: string, direction: PortDirection): PortSpec {
  const spec = CATALOG.nodes
    .find((entry) => entry.kind === kind)
    ?.ports.find(
      (port) =>
        port.direction === direction &&
        (port.name === name ||
          ((port.repeat ?? "once") !== "once" && portStream(port.name, name) !== null)),
    );
  const portType = spec?.port_type ?? "events";
  return {
    name,
    direction,
    port_type: portType,
    payload: spec?.payload ?? payloadOf(portType),
    multi: spec?.multi ?? "many",
  };
}

export interface Wiring {
  devices?: ReadonlyMap<string, DeviceSet>;
  channels?: ReadonlyMap<string, ChannelInfo>;
  owners?: ReadonlyMap<string, string>;
  runs?: Readonly<Record<string, Runs>>;
}

function ownerOf(graph: PatchGraph, node: string, wiring: Wiring): DeviceSet | undefined {
  const owner =
    wiring.owners?.get(node) ??
    (graph.edges ?? []).find((edge) => edge.to.node === node && wiring.devices?.has(edge.from.node))
      ?.from.node;
  return owner === undefined ? undefined : wiring.devices?.get(owner);
}

function runsOf(graph: PatchGraph, node: string, wiring: Wiring): Runs {
  const given = wiring.runs?.[node];
  if (given !== undefined) {
    return given;
  }
  const radio = wiring.devices?.get(node);
  if (radio !== undefined) {
    return { runs: "radio", device_set: radio.id };
  }
  const channel = wiring.channels?.get(node);
  const set = ownerOf(graph, node, wiring);
  return channel !== undefined && set !== undefined
    ? { runs: "channel", device_set: set.id, channel: channel.id, stream: channel.stream ?? 0 }
    : { runs: "server" };
}

function tapOf(runs: Runs | undefined, port: string): Tap | undefined {
  if (runs?.runs === "channel") {
    return { tap: "channel", device_set: runs.device_set, channel: runs.channel };
  }
  const stream = runs?.runs === "radio" ? portStream("iq", port) : null;
  return runs?.runs === "radio" && stream !== null
    ? { tap: "lane", device_set: runs.device_set, stream }
    : undefined;
}

function catalogPortsOf(kind: NodeKind): Map<string, ResolvedPort> {
  return new Map(
    (CATALOG.nodes.find((entry) => entry.kind === kind)?.ports ?? []).map((spec) => [
      `${spec.direction}:${spec.name}`,
      { ...spec, repeat: "once" as const },
    ]),
  );
}

const MAX_PASS_DEPTH = 16;

function passes(graph: PatchGraph, node: string, portType: string): boolean {
  const kind = graph.nodes.find((candidate) => candidate.id === node)?.kind;
  const role = CATALOG.nodes.find((entry) => entry.kind === kind)?.role;
  return role?.role === "pass" && role.of === portType;
}

function feedsInto(
  graph: PatchGraph,
  runs: ReadonlyMap<string, Runs>,
  to: { node: string; port: string },
  portType: string,
  via: readonly string[],
): Feed[] {
  return (graph.edges ?? [])
    .filter((edge) => edge.to.node === to.node && edge.to.port === to.port)
    .flatMap((edge) => {
      const source = edge.from.node;
      if (!passes(graph, source, portType)) {
        const tap = tapOf(runs.get(source), edge.from.port);
        const kind = graph.nodes.find((candidate) => candidate.id === source)?.kind;
        const payload =
          kind === undefined
            ? payloadOf(portType as PortType)
            : specFor(kind, edge.from.port, "out").payload;
        return [
          {
            source,
            port: edge.from.port,
            payload,
            ...(via.length === 0 ? {} : { via: [...via] }),
            ...(tap === undefined ? {} : { tap }),
          },
        ];
      }
      if (via.length >= MAX_PASS_DEPTH || via.includes(source)) {
        return [];
      }
      return (graph.edges ?? [])
        .filter((inner) => inner.to.node === source)
        .map((inner) => inner.to.port)
        .filter((port, index, all) => all.indexOf(port) === index)
        .flatMap((port) =>
          feedsInto(graph, runs, { node: source, port }, portType, [source, ...via]),
        );
    });
}

export function wiredPatch(graph: PatchGraph, wiring: Wiring = {}): ResolvedPatch {
  const runs = new Map(graph.nodes.map((node) => [node.id, runsOf(graph, node.id, wiring)]));
  return resolvedPatch(
    graph.nodes.map((node) => {
      const ports = catalogPortsOf(node.kind);
      const port = (name: string, direction: PortDirection): ResolvedPort => {
        const key = `${direction}:${name}`;
        const held = ports.get(key) ?? { ...specFor(node.kind, name, direction) };
        ports.set(key, held);
        return held;
      };
      for (const edge of graph.edges ?? []) {
        if (edge.to.node === node.id) {
          const input = port(edge.to.port, "in");
          input.feeds ??= feedsInto(graph, runs, edge.to, input.port_type, []);
        }
        if (edge.from.node === node.id) {
          const output = port(edge.from.port, "out");
          output.reaches = [...(output.reaches ?? []), { sink: edge.to.node, port: edge.to.port }];
        }
      }
      const resolved = resolvedNodeOf(node.id, node.kind, [...ports.values()], runs.get(node.id));
      return node.label == null ? resolved : { ...resolved, label: node.label };
    }),
  );
}
