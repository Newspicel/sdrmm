import { pushToast } from "../lib/toasts";
import type { NodeKind, PatchNode, Position } from "../lib/types";
import { useWorkspaceNow } from "./context";
import { addNode, newNodeId, nodeIds, opensDevice } from "./graph";
import { newNodeBody } from "./newNode";
import { useNodePlacement } from "./placement";

/** Adds a node of `kind`; `typeId` picks the decoder, encoder or plugin processor it runs. A
 * channel without one starts on the first decoder. */
export type AddNode = (kind: NodeKind, typeId?: string, at?: Position) => void;

export function useAddNode(): AddNode {
  const now = useWorkspaceNow();
  const placeNode = useNodePlacement();

  return (kind, typeId, at) => {
    const workspace = now();
    const firstDecoder =
      kind === "channel" ? workspace.context.channelTypes[0]?.type_id : undefined;
    const body = newNodeBody(workspace.context.catalog, kind, { typeId: typeId ?? firstDecoder });
    if (body === null) {
      pushToast(`Unknown node: ${kind}`);
      return;
    }
    const id = newNodeId(kind, nodeIds(workspace.graph));
    const fresh = { id, position: { x: 0, y: 0 }, ...body } as PatchNode;
    const node = { ...fresh, position: at ?? placeNode(workspace.graph, fresh) };
    workspace.edit((snapshot) => ({ ...snapshot, graph: addNode(snapshot.graph, node) }));
    workspace.select(id);
    workspace.inspect(id);
    if (opensDevice(kind)) {
      workspace.apply();
    }
  };
}
