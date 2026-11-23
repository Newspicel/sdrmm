import { Chips, ChoiceChip } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { Readout, Readouts } from "../../components/face/Readouts";
import { FaceStats, Stat } from "../../components/face/Stats";
import { TextChip } from "../../components/face/TextChip";
import { DROPS_HINT, formatBytes, formatCount } from "../../components/format";
import type { PatchNode, PatchNodeOf } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { hasFeed } from "../lanes";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell } from "./NodeShell";
import { pictureSize, streamUrl, useStreamOutStatus } from "./streamOut";

const TRANSPORTS = [
  { value: "http", label: "HTTP" },
  { value: "udp", label: "UDP" },
  { value: "rtp", label: "RTP" },
] as const;

export function StreamOutFace({ node }: { node: PatchNode }) {
  if (node.kind !== "stream_out") {
    return null;
  }
  return <StreamOutNodeFace node={node} />;
}

function StreamOutNodeFace({ node }: { node: PatchNodeOf<"stream_out"> }) {
  const workspace = useWorkspaceContext();
  const status = useStreamOutStatus(node.id);
  const wired = hasFeed(workspace.resolved, node.id, "video");
  const sends = node.data.transport !== "http";
  const edit = (next: Partial<typeof node.data>) => {
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "stream_out"
          ? { ...current, data: { ...current.data, ...next } }
          : current,
      ),
    }));
  };
  const url = status?.path == null ? "—" : streamUrl(status.path, window.location.origin);
  return (
    <NodeShell node={node} title="Stream out" category="output">
      <Inspect node={node.id}>
        <InspectSection>
          <Chips inset="none">
            <ChoiceChip
              label="Transport"
              title="HTTP serves players that open the URL; UDP and RTP send to one address"
              value={node.data.transport}
              options={TRANSPORTS}
              onChange={(transport) => edit({ transport })}
            />
            {sends && (
              <TextChip
                label="To"
                name="Stream destination"
                title="Destination host:port, unicast or multicast"
                value={node.data.address}
                onCommit={(address) => {
                  if (address !== "") {
                    edit({ address });
                  }
                }}
              />
            )}
          </Chips>
        </InspectSection>
      </Inspect>
      <FaceBody>
        {!wired ? (
          <FaceEmpty />
        ) : (
          <>
            <Readouts>
              <Readout label={sends ? "To" : "URL"} title={sends ? node.data.address : url} wide>
                <span className="select-all">{sends ? node.data.address : url}</span>
              </Readout>
              <Readout label="Picture" title="Coded size, MPEG-2 at 25 frames a second">
                {pictureSize(status)}
              </Readout>
            </Readouts>
            {status?.error != null && <FaceFault message={status.error} />}
          </>
        )}
      </FaceBody>
      {wired && (
        <FaceFooter>
          <FaceStats>
            {!sends && (
              <Stat label="Viewers" title="Players watching">
                {status?.clients ?? 0}
              </Stat>
            )}
            <Stat label="Sent" title="Bytes sent">
              {formatBytes(status?.bytes ?? 0)}
            </Stat>
            {(status?.dropped ?? 0) > 0 && (
              <Stat label="Drops" title={DROPS_HINT} tone="warn">
                {formatCount(status?.dropped ?? 0)}
              </Stat>
            )}
          </FaceStats>
        </FaceFooter>
      )}
    </NodeShell>
  );
}
