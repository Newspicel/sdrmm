import { DownloadMenu } from "../../components/DownloadMenu";
import { DEFAULT_LOG_FILTER, logDownloads, toQuery } from "../../components/decoderLog";
import type { PatchNode } from "../../lib/types";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell } from "./NodeShell";
import { useWireScope } from "./sinkInputs";

export function ExportFace({ node }: { node: PatchNode }) {
  const wires = useWireScope(node.id);
  return (
    <NodeShell node={node} title="Export" category="output">
      <FaceBody>
        <FaceEmpty hint={!wires.wired ? "Wire decoders in" : "Every logged row, as one file"} />
      </FaceBody>
      <FaceFooter>
        <DownloadMenu
          choices={logDownloads(toQuery(DEFAULT_LOG_FILTER, wires))}
          disabled={!wires.wired}
        />
      </FaceFooter>
    </NodeShell>
  );
}
