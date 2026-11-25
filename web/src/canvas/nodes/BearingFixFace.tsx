import { Readout, Readouts } from "../../components/face/Readouts";
import { useFeeds } from "../../lib/resolved";
import type { PatchNode } from "../../lib/types";
import { FaceBody, NodeShell } from "./NodeShell";

export function BearingFixFace({ node }: { node: PatchNode }) {
  const sources = useFeeds(node.id, "events").length;
  return (
    <NodeShell node={node} title="Bearing fix" category="tool">
      <FaceBody>
        <Readouts>
          <Readout label="Sources" title="Wires bringing bearings in; the fix goes out on events">
            {sources}
          </Readout>
        </Readouts>
      </FaceBody>
    </NodeShell>
  );
}
