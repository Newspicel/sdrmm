import type { DeviceSet, PatchGraph, PatchNode, RadioActivity, RadioUse } from "../../lib/types";
import { type GraphContext, nodeOf, nodePorts, patchNode } from "../graph";

export const USE_LABEL: Record<RadioUse, string> = {
  receive: "Receive",
  send: "Send",
  both: "Both",
};

/** What a Device node uses its radio for: its setting, unless the open radio cannot do that. */
export function radioUseOf(node: PatchNode | undefined, set: DeviceSet | null): RadioUse {
  const wanted = node?.kind === "device" ? (node.data.radio_use ?? "receive") : "receive";
  if (set === null || (set.uses ?? []).includes(wanted)) {
    return wanted;
  }
  return set.radio_use ?? wanted;
}

export function activityOf(set: DeviceSet): RadioActivity {
  return set.activity ?? "idle";
}

/** Puts a Device node to `radioUse` and drops the wires on the ports that use leaves out. */
export function withRadioUse(
  graph: PatchGraph,
  context: GraphContext,
  id: string,
  radioUse: RadioUse,
): PatchGraph {
  const found = nodeOf(graph, id);
  if (found?.kind !== "device") {
    return graph;
  }
  const ports = nodePorts(context, found);
  const leftOut = (node: string, port: string): boolean => {
    const uses = ports.find((spec) => spec.name === port)?.uses ?? [];
    return node === id && uses.length > 0 && !uses.includes(radioUse);
  };
  const used = patchNode(graph, id, () => ({
    ...found,
    data: { ...found.data, radio_use: radioUse },
  }));
  return {
    ...used,
    edges: (graph.edges ?? []).filter(
      (edge) => !leftOut(edge.from.node, edge.from.port) && !leftOut(edge.to.node, edge.to.port),
    ),
  };
}
