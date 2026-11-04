import type { PatchNode } from "../../lib/types";
import { FaceBody, FaceEmpty, NodeShell } from "./NodeShell";
import { useWireScope } from "./sinkInputs";

export function BearingFixFace({ node }: { node: PatchNode }) {
  const wires = useWireScope(node.id);
  return (
    <NodeShell node={node} title="Bearing fix" category="tool">
      <FaceBody>
        <FaceEmpty hint={wires.wired ? "Fix goes out on events" : "Wire bearings in"} />
      </FaceBody>
    </NodeShell>
  );
}
