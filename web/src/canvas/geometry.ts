import type { PatchGraph, PatchNode, Position, Size } from "../lib/types";
import { isResizable, NODE_SIZE, nodeOf } from "./graph";

export interface Geometry {
  position: Position;
  size: Size | null;
}

export interface Placed {
  id: string;
  position: Position;
  width?: number | null;
  height?: number | null;
}

function geometryOf(placed: Placed, node: PatchNode): Geometry {
  const natural = NODE_SIZE[node.kind];
  const { width: w, height: h } = placed;
  const resized =
    isResizable(node.kind) && w != null && h != null && (w !== natural.w || h !== natural.h);
  return {
    position: { x: placed.position.x, y: placed.position.y },
    size: resized ? { w, h } : null,
  };
}

function sameGeometry(node: PatchNode, geometry: Geometry): boolean {
  const size = node.size ?? null;
  return (
    node.position.x === geometry.position.x &&
    node.position.y === geometry.position.y &&
    size?.w === geometry.size?.w &&
    size?.h === geometry.size?.h
  );
}

export function movedGeometry(
  placed: readonly Placed[],
  graph: PatchGraph,
): ReadonlyMap<string, Geometry> {
  const moved = new Map<string, Geometry>();
  for (const held of placed) {
    const node = nodeOf(graph, held.id);
    if (node === undefined) {
      continue;
    }
    const geometry = geometryOf(held, node);
    if (!sameGeometry(node, geometry)) {
      moved.set(held.id, geometry);
    }
  }
  return moved;
}

export function withGeometry(graph: PatchGraph, moved: ReadonlyMap<string, Geometry>): PatchGraph {
  return {
    ...graph,
    nodes: graph.nodes.map((node) => {
      const geometry = moved.get(node.id);
      if (geometry === undefined) {
        return node;
      }
      const { size: _dropped, ...rest } = node;
      return {
        ...rest,
        position: geometry.position,
        ...(geometry.size === null ? {} : { size: geometry.size }),
      };
    }),
  };
}
