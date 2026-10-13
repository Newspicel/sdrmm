import { type Edge, MarkerType } from "@xyflow/react";
import type { PatchGraph } from "../lib/types";
import { HUE_COLOR, portHue } from "../sinks/hues";
import { edgeKey, type GraphContext, portOf } from "./graph";

export const RETUNES_TITLE = "Retunes its radio";

export function toFlowEdges(graph: PatchGraph, context: GraphContext): Edge[] {
  const retuning = new Set(
    (context.resolved?.edges ?? []).filter((edge) => edge.retunes_source).map(edgeKey),
  );
  return (graph.edges ?? []).map((edge) => {
    const carried = portOf(context, graph, edge.from, "out")?.port_type;
    const stroke = carried === undefined ? undefined : HUE_COLOR[portHue(context.catalog, carried)];
    const retunes = retuning.has(edgeKey(edge));
    return {
      id: edgeKey(edge),
      source: edge.from.node,
      sourceHandle: edge.from.port,
      target: edge.to.node,
      targetHandle: edge.to.port,
      className: retunes ? `wire-${carried} wire-retunes` : `wire-${carried}`,
      ...(stroke === undefined ? {} : { style: { stroke } }),
      ...(retunes
        ? {
            markerStart: { type: MarkerType.ArrowClosed, color: stroke },
            ariaLabel: RETUNES_TITLE,
          }
        : {}),
    };
  });
}
