import type { PatchNode } from "../../lib/types";
import { MapView } from "../../sinks/MapView";
import { useWorkspaceContext } from "../context";
import { FaceBody, NodeShell, useFaceActive } from "./NodeShell";

export function MapFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const active = useFaceActive();
  const sourceName = (source: string): string =>
    workspace.graph.nodes.find((found) => found.id === source)?.label ?? source;
  return (
    <NodeShell node={node} title="Map" category="output">
      <FaceBody scroll={false}>
        <MapView node={node.id} sourceName={sourceName} active={active} />
      </FaceBody>
    </NodeShell>
  );
}
