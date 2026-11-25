import { MapSettings } from "../../components/map/MapSettings";
import type { PatchNode } from "../../lib/types";
import { MapView } from "../../sinks/MapView";
import { useWorkspaceSelect, type Workspace } from "../context";
import { Inspect } from "../inspector";
import { FaceBody, NodeShell, useFaceActive } from "./NodeShell";

export function MapFace({ node }: { node: PatchNode }) {
  const labels = useWorkspaceSelect(nodeLabels);
  const active = useFaceActive();
  const sourceName = (source: string): string => labels[source] ?? source;
  return (
    <NodeShell node={node} title="Map" category="output">
      <Inspect node={node.id}>
        <MapSettings />
      </Inspect>
      <FaceBody scroll={false}>
        <MapView node={node.id} sourceName={sourceName} active={active} />
      </FaceBody>
    </NodeShell>
  );
}

function nodeLabels(workspace: Workspace): Record<string, string> {
  const labels: Record<string, string> = {};
  for (const node of workspace.graph.nodes) {
    if (node.label != null) {
      labels[node.id] = node.label;
    }
  }
  return labels;
}
