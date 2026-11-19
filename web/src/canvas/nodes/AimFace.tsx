import { useAimStore } from "../../lib/pointing";
import type { Field, PatchNode } from "../../lib/types";
import { NodeForm, StatusGrid } from "./NodeForm";
import { FaceBody, FaceStanding, NodeShell } from "./NodeShell";
import { useUnwired, type Want } from "./needs";

const NO_READOUTS: readonly Field[] = [];
const WANTS: readonly Want[] = [{ port: "events" }, { port: "position" }];

export function AimFace({ node }: { node: PatchNode }) {
  const status = useAimStore((store) => store.byNode[node.id]);
  const unwired = useUnwired(node.id, WANTS);
  if (node.kind !== "aim") {
    return null;
  }
  return (
    <NodeShell node={node} title="Aim" category="tool" live={(status?.driving ?? []).length > 0}>
      <FaceBody>
        <NodeForm node={node} />
        <StatusGrid readouts={status?.readouts ?? NO_READOUTS} />
        <FaceStanding unwired={unwired} error={status?.error} waiting={status?.waiting} />
      </FaceBody>
    </NodeShell>
  );
}
