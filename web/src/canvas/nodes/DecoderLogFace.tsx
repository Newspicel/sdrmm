import { DecoderLogPanel } from "../../components/DecoderLogPanel";
import type { LogGroupKey, PatchNode } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { NodeShell } from "./NodeShell";
import { useWireScope } from "./sinkInputs";

export function DecoderLogFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const wires = useWireScope(node.id);
  const group = node.kind === "decoder_log" ? (node.data?.group ?? null) : null;
  const remember = (next: LogGroupKey | null): void => {
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "decoder_log"
          ? { ...current, data: { ...current.data, group: next } }
          : current,
      ),
    }));
  };
  return (
    <NodeShell node={node} title="Decoder log" category="output">
      <DecoderLogPanel wires={wires} group={group} onGroup={remember} />
    </NodeShell>
  );
}
