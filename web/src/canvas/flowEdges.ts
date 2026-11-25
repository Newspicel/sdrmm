import type { Edge } from "@xyflow/react";
import type { PatchGraph } from "../lib/types";
import { HUE_COLOR, portHue } from "../sinks/hues";
import { edgeKey, type GraphContext, portOf } from "./graph";

/** Keeps each wire that did not change as the same object, so React Flow leaves it alone. */
export function keptEdges(previous: Edge[], next: readonly Edge[]): Edge[] {
  const held = new Map(previous.map((edge) => [edge.id, edge]));
  const kept = next.map((edge) => {
    const old = held.get(edge.id);
    return old !== undefined &&
      old.className === edge.className &&
      old.style?.stroke === edge.style?.stroke
      ? old
      : edge;
  });
  return kept.length === previous.length && kept.every((edge, i) => edge === previous[i])
    ? previous
    : kept;
}

export function toFlowEdges(graph: PatchGraph, context: GraphContext): Edge[] {
  return (graph.edges ?? []).map((edge) => {
    const carried = portOf(context, graph, edge.from, "out")?.port_type;
    const stroke = carried === undefined ? undefined : HUE_COLOR[portHue(context.catalog, carried)];
    return {
      id: edgeKey(edge),
      source: edge.from.node,
      sourceHandle: edge.from.port,
      target: edge.to.node,
      targetHandle: edge.to.port,
      className: `wire-${carried}`,
      ...(stroke === undefined ? {} : { style: { stroke } }),
    };
  });
}
