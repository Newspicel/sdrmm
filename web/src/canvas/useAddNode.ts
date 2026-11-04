import { pushToast } from "../lib/toasts";
import type { NodeKind, PatchNode, Position } from "../lib/types";
import { useWorkspaceContext } from "./context";
import { addNode, newNodeId, nodeIds, opensDevice } from "./graph";
import { newNodeBody } from "./newNode";
import { useNodePlacement } from "./placement";

/** Adds a node of `kind`; `typeId` picks the decoder or plugin processor it runs. */
export type AddNode = (kind: NodeKind, typeId?: string, at?: Position) => void;

export function useAddNode(): AddNode {
  const workspace = useWorkspaceContext();
  const placeNode = useNodePlacement();

  return (kind, typeId, at) => {
    const body = newNodeBody(workspace.context.catalog, kind, {
      channelType: typeId ?? workspace.context.channelTypes[0]?.type_id,
      processorType: typeId,
    });
    if (body === null) {
      pushToast(`Unknown node: ${kind}`);
      return;
    }
    const id = newNodeId(kind, nodeIds(workspace.graph));
    const node = { id, position: at ?? placeNode(workspace.graph, kind), ...body } as PatchNode;
    workspace.edit((snapshot) => ({ ...snapshot, graph: addNode(snapshot.graph, node) }));
    workspace.select(id);
    if (opensDevice(kind)) {
      workspace.apply();
    }
  };
}
