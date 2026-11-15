import { FaceFault } from "../../components/face/Fault";
import { useAimStore } from "../../lib/pointing";
import type { Field, PatchNode } from "../../lib/types";
import { NodeForm, StatusGrid } from "./NodeForm";
import { FaceBody, NodeShell } from "./NodeShell";

const NO_READOUTS: readonly Field[] = [];

export function AimFace({ node }: { node: PatchNode }) {
  const status = useAimStore((store) => store.byNode[node.id]);
  if (node.kind !== "aim") {
    return null;
  }
  return (
    <NodeShell node={node} title="Aim" category="tool" live={(status?.driving ?? []).length > 0}>
      <FaceBody>
        <NodeForm node={node} />
        <StatusGrid readouts={status?.readouts ?? NO_READOUTS} />
        {status?.error != null && <FaceFault message={status.error} />}
      </FaceBody>
    </NodeShell>
  );
}
