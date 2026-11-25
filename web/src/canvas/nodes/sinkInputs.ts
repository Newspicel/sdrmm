import type { WireScope } from "../../components/decoderLog";
import { feedSources, type Input } from "../../lib/resolved";
import type { PatchGraph } from "../../lib/types";
import { useWorkspaceSelect } from "../context";

export function inputKey(input: Input): string {
  return input.source;
}

export function useInputName(): (input: Input) => string {
  const labels = useWorkspaceSelect((workspace) => nodeLabels(workspace.graph));
  return (input) =>
    labels[input.node] ?? input.channel?.settings.params.type.toUpperCase() ?? input.node;
}

function nodeLabels(graph: PatchGraph): Record<string, string> {
  const labels: Record<string, string> = {};
  for (const node of graph.nodes) {
    if (node.label != null) {
      labels[node.id] = node.label;
    }
  }
  return labels;
}

export function useWireScope(sink: string): WireScope {
  const wired = useWorkspaceSelect(
    (workspace) => feedSources(workspace.resolved, sink, "events").length > 0,
  );
  return { sink, wired };
}
