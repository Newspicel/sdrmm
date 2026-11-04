import { DecoderLogPanel } from "../../components/DecoderLogPanel";
import { DEFAULT_LOG_FILTER } from "../../components/decoderLog";
import type { PatchNode } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { NodeShell } from "./NodeShell";
import { useWireScope } from "./sinkInputs";

type LogData = Extract<PatchNode, { kind: "decoder_log" }>["data"];

export function DecoderLogFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const wires = useWireScope(node.id);
  const data = node.kind === "decoder_log" ? node.data : undefined;
  const remember = (next: Partial<NonNullable<LogData>>): void => {
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "decoder_log"
          ? { ...current, data: { ...current.data, ...next } }
          : current,
      ),
    }));
  };
  return (
    <NodeShell node={node} title="Decoder log" category="output">
      <DecoderLogPanel
        wires={wires}
        group={data?.group ?? null}
        onGroup={(group) => remember({ group })}
        limit={data?.limit ?? DEFAULT_LOG_FILTER.limit}
        onLimit={(limit) => remember({ limit })}
      />
    </NodeShell>
  );
}
