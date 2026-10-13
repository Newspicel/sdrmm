import { HuntPanel } from "../../components/HuntPanel";
import { feedsOf } from "../../lib/resolved";
import type { PatchNode, PatchNodeOf } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { hasFeed } from "../lanes";
import { huntSweepOf } from "../newNode";
import { decoderOf } from "../workspaceDevice";
import { NodeShell } from "./NodeShell";

export function HuntFace({ node }: { node: PatchNode }) {
  if (node.kind !== "hunt") {
    return null;
  }
  return <HuntNodeFace node={node} />;
}

function HuntNodeFace({ node }: { node: PatchNodeOf<"hunt"> }) {
  const workspace = useWorkspaceContext();
  const decoder = decoderOf(workspace, node.id);
  const sweep = huntSweepOf(node, workspace.context.catalog);
  const scanning =
    decoder !== null &&
    feedsOf(workspace.resolved, decoder.node, "control").some((feed) =>
      decoder.set.scanners?.some((scanner) => scanner.settings.node === feed.source),
    );
  const remember = (data: Partial<PatchNodeOf<"hunt">["data"]>): void => {
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "hunt" ? { ...current, data: { ...current.data, ...data } } : current,
      ),
    }));
  };
  return (
    <NodeShell node={node} title="Signal hunt" category="tool">
      <HuntPanel
        node={node.id}
        target={decoder}
        clicks={node.data.clicks ?? true}
        onClicks={(clicks) => remember({ clicks })}
        hint="Wire this node's control out to a decoder"
        scanning={scanning}
        positionWired={hasFeed(workspace.resolved, node.id, "position")}
        sweep={sweep}
        onSweep={(next) => {
          if (sweep !== null) {
            remember({ sweep: { ...sweep, ...next } });
          }
        }}
      />
    </NodeShell>
  );
}
