import type {
  Feed,
  NodeKind,
  Payload,
  PortSpec,
  PortType,
  Reach,
  ResolvedNode,
  ResolvedPatch,
  ResolvedPort,
  Runs,
} from "../lib/types";

export function payloadOf(portType: PortType): Payload {
  switch (portType) {
    case "iq":
      return { type: "iq", origins: ["radio"] };
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

export function portName(base: string, index: number): string {
  return index === 0 ? base : `${base}${index + 1}`;
}
