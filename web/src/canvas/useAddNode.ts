import { pushToast } from "../lib/toasts";
import type { NodeKind, PatchNode, Position } from "../lib/types";
import { useWorkspaceContext } from "./context";
import { addNode, newNodeId, nodeIds, opensDevice } from "./graph";
import { newNodeBody } from "./newNode";
import { useNodePlacement } from "./placement";

export type AddNode = (kind: NodeKind, channelType?: string, at?: Position) => void;

export function useAddNode(): AddNode {
  const workspace = useWorkspaceContext();
  const placeNode = useNodePlacement();

  return (kind, channelType, at) => {
    const body = newNodeBody(workspace.context.catalog, kind, { channelType });
    if (body === null) {
      pushToast(`Unknown node: ${kind}`);
      return;
    }
    const id = newNodeId(kind, nodeIds(workspace.graph));
    workspace.edit((snapshot) => {
      const node = { id, position: at ?? placeNode(snapshot.graph, kind), ...body } as PatchNode;
      return { ...snapshot, graph: addNode(snapshot.graph, node) };
    });
    workspace.select(id);
    if (opensDevice(kind)) {
      workspace.apply();
    }
  };
}
