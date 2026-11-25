import { useFeeds } from "../../lib/resolved";
import { useSinkFeed } from "../../lib/sinks";
import type { PatchNode } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { ReadoutView } from "../../sinks/ReadoutView";
import { useWorkspaceSelect, type Workspace } from "../context";
import { FaceBody, FaceWell, NodeShell } from "./NodeShell";

export function ReadoutFace({ node }: { node: PatchNode }) {
  const labels = useWorkspaceSelect(nodeLabels);
  const feeds = useFeeds(node.id, "events");
  const feed = useSinkFeed(node.id);
  const now = useNow(1000);
  const sourceName = (source: string): string => labels[source] ?? source;
  return (
    <NodeShell node={node} title="Readout" category="output">
      <FaceBody>
        {feeds.length === 0 ? (
          <FaceWell />
        ) : (
          <ReadoutView feed={feed} sourceName={sourceName} now={now} />
        )}
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
