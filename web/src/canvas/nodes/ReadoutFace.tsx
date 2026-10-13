import { PANELS } from "../../components/DecoderPanels";
import { useFeeds } from "../../lib/resolved";
import { useSinkFeed } from "../../lib/sinks";
import type { PatchNode } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { ReadoutView } from "../../sinks/ReadoutView";
import { useWorkspaceContext } from "../context";
import { FaceBody, FaceEmpty, NodeShell } from "./NodeShell";

export function ReadoutFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const feeds = useFeeds(node.id, "events");
  const feed = useSinkFeed(node.id);
  const now = useNow(1000);
  const sourceName = (source: string): string => {
    const found = workspace.graph.nodes.find((n) => n.id === source);
    return found?.label ?? source;
  };
  return (
    <NodeShell node={node} title="Readout" category="output">
      <FaceBody>
        {feeds.length === 0 ? (
          <FaceEmpty hint="Wire a decoder's events in" />
        ) : (
          <ReadoutView feed={feed} panels={PANELS} sourceName={sourceName} now={now} />
        )}
      </FaceBody>
    </NodeShell>
  );
}
