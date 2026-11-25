import type { ReactNode } from "react";
import { AudioControls } from "../../components/AudioControls";
import { audioChainActive } from "../../components/channelSettings";
import { Chips } from "../../components/face/Chips";
import type { AudioProcessing, PatchNode, PortSpec } from "../../lib/types";
import { useWorkspaceNow } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";

/** The Audio FX chain on a node's audio output, or undefined for a node that has none. */
export function audioFxOf(
  node: PatchNode,
  ports: readonly PortSpec[],
): AudioProcessing | undefined {
  if (!ports.some((port) => port.direction === "out" && port.port_type === "audio")) {
    return undefined;
  }
  return "data" in node ? ((node.data as { audio?: AudioProcessing }).audio ?? {}) : {};
}

export function withAudioFxBadge(badge: ReactNode, audio: AudioProcessing | undefined): ReactNode {
  if (!audioChainActive(audio)) {
    return badge;
  }
  return (
    <span className="flex gap-1.5">
      {badge}
      <span title="Audio processing on">FX</span>
    </span>
  );
}

function withAudio(node: PatchNode, audio: AudioProcessing): PatchNode {
  return "data" in node ? ({ ...node, data: { ...node.data, audio } } as PatchNode) : node;
}

export function AudioFxInspect({
  node,
  audio,
}: {
  node: PatchNode;
  audio: AudioProcessing | undefined;
}) {
  const now = useWorkspaceNow();
  if (audio === undefined) {
    return null;
  }
  const edit = (next: AudioProcessing): void =>
    now().edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) => withAudio(current, next)),
    }));
  return (
    <Inspect node={node.id}>
      <InspectSection label="Audio FX">
        <Chips inset="none">
          <AudioControls audio={audio} onAudio={edit} />
        </Chips>
      </InspectSection>
    </Inspect>
  );
}
