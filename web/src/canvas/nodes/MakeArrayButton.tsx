import type { MouseEvent } from "react";
import { Button } from "../../components/BaseControls";
import { FACE_BTN } from "../../components/controls";
import { resolvedNode } from "../../lib/resolved";
import { pushToast } from "../../lib/toasts";
import { useWorkspaceNow, useWorkspaceSelect } from "../context";
import { newNodeId, nodeIds } from "../graph";
import { revealWhenInspected } from "../KeepInView";
import { MAKE_ARRAY_TITLE, makeArray } from "../makeArray";
import { defaultBody } from "../newNode";

export function MakeArrayButton({ node }: { node: string }) {
  const now = useWorkspaceNow();
  const offer = useWorkspaceSelect(
    (workspace) => resolvedNode(workspace.resolved, node)?.array_offer,
  );
  if (offer == null) {
    return null;
  }
  const make = (event: MouseEvent): void => {
    event.stopPropagation();
    const workspace = now();
    const body = defaultBody(workspace.context.catalog, "array");
    if (body?.kind !== "array") {
      pushToast("Unknown node: array");
      return;
    }
    if (offer.offer !== "ready") {
      return;
    }
    const id = newNodeId("array", nodeIds(workspace.graph));
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: makeArray(snapshot.graph, node, offer.wires, body, id),
    }));
    workspace.select(id);
    workspace.inspect(id);
    revealWhenInspected(id);
    workspace.apply();
  };
  const ready = offer.offer === "ready";
  const title = ready ? MAKE_ARRAY_TITLE : offer.reason;
  const button = (
    <Button type="button" className={FACE_BTN} title={title} disabled={!ready} onClick={make}>
      Make array
    </Button>
  );
  return ready ? button : <span title={title}>{button}</span>;
}
