import type { LaneWire, NodeBodyOf, PatchGraph, PatchNode, Position } from "../lib/types";
import { addEdge, addNode, nodeOf, nodeSize } from "./graph";

export const MAKE_ARRAY_TITLE = "New Array wired to every lane";

const ARRAY_GAP_PX = 80;

export function arrayPlacement(graph: PatchGraph, device: string): Position {
  const source = nodeOf(graph, device);
  const position = source?.position ?? { x: 0, y: 0 };
  return { x: position.x + nodeSize(source?.kind ?? "device").w + ARRAY_GAP_PX, y: position.y };
}

export function makeArray(
  graph: PatchGraph,
  device: string,
  wires: readonly LaneWire[],
  body: NodeBodyOf<"array">,
  id: string,
): PatchGraph {
  const array: PatchNode = { id, position: arrayPlacement(graph, device), ...body };
  return wires.reduce(
    (next, wire) =>
      addEdge(next, { from: { node: device, port: wire.from }, to: { node: id, port: wire.to } }),
    addNode(graph, array),
  );
}
