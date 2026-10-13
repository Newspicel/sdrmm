import { forgetNodes } from "../lib/nodeState";
import type { Workspace } from "./context";
import { pruneRack, removeNode } from "./graph";

export function dropNodes(workspace: Pick<Workspace, "edit">, ids: readonly string[]): void {
  if (ids.length === 0) {
    return;
  }
  workspace.edit((snapshot) => {
    const graph = ids.reduce((held, id) => removeNode(held, id), snapshot.graph);
    return { ...snapshot, graph, rack: pruneRack(snapshot.rack ?? {}, graph) };
  });
  forgetNodes(ids);
}
