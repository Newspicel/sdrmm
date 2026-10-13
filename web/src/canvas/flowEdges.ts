import type { Edge } from "@xyflow/react";
import type { PatchGraph } from "../lib/types";
import { HUE_COLOR, portHue } from "../sinks/hues";
import { edgeKey, type GraphContext, portOf } from "./graph";

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
