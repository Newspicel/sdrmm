import { useQuery } from "@tanstack/react-query";
import { DownloadMenu } from "../../components/DownloadMenu";
import { DEFAULT_LOG_FILTER, logDownloads, toQuery } from "../../components/decoderLog";
import { Readout, Readouts } from "../../components/face/Readouts";
import { formatCount } from "../../components/format";
import { decoderLogQuery } from "../../lib/api";
import type { PatchNode } from "../../lib/types";
import { FaceBody, FaceFooter, NodeShell } from "./NodeShell";
import { useWireScope } from "./sinkInputs";

/** One row is enough to learn how many the log holds. */
const COUNT_ONLY = { ...DEFAULT_LOG_FILTER, limit: 1 };

export function ExportFace({ node }: { node: PatchNode }) {
  const wires = useWireScope(node.id);
  const stored = useQuery({
    ...decoderLogQuery(toQuery(COUNT_ONLY, wires)),
    enabled: wires.wired,
  });
  const total = wires.wired ? stored.data?.total : undefined;
  return (
    <NodeShell node={node} title="Export" category="output">
      <FaceBody>
        <Readouts>
          <Readout label="Rows" title="Logged rows the download holds">
            {total === undefined ? null : formatCount(total)}
          </Readout>
        </Readouts>
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
