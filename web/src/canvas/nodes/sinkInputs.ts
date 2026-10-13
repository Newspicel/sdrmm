import type { WireScope } from "../../components/decoderLog";
import { feedSources, type Input } from "../../lib/resolved";
import { useWorkspaceContext } from "../context";

export function inputKey(input: Input): string {
  return input.source;
}

export function useInputName(): (input: Input) => string {
  const workspace = useWorkspaceContext();
  return (input) =>
    workspace.graph.nodes.find((n) => n.id === input.node)?.label ??
    input.channel.settings.params.type.toUpperCase();
}

export function useWireScope(sink: string): WireScope {
  const workspace = useWorkspaceContext();
  return { sink, wired: feedSources(workspace.resolved, sink, "events").length > 0 };
}
