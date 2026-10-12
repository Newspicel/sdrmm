import catalog from "../generated/catalog-fixture.json";
import type { NodeBody, NodeKind, NodeTypeInfo, PatchCatalog, PortSpec } from "../lib/types";

export const CATALOG = catalog as unknown as PatchCatalog;

function entryOf(kind: NodeKind): NodeTypeInfo {
  const entry = CATALOG.nodes.find((type) => type.kind === kind);
  if (entry === undefined) {
    throw new Error(`the catalog has no ${kind}`);
  }
  return entry;
}

export function catalogBody(kind: NodeKind): NodeBody {
  return structuredClone(entryOf(kind).default_body);
}

export function catalogShape(kind: NodeKind): Pick<NodeTypeInfo, "role" | "face"> {
  const { role, face } = entryOf(kind);
  return { role, face };
}

export function channelPortsNamed(...names: string[]): PortSpec[] {
  return entryOf("channel").ports.filter((port) => names.includes(port.name));
}
