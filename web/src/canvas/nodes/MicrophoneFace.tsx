import { useState } from "react";
import { Button } from "../../components/BaseControls";
import { BTN, BTN_DANGER_ARMED } from "../../components/controls";
import { Chips } from "../../components/face/Chips";
import { MeterBar } from "../../components/face/Meter";
import { VOICE_CHAT_LIMITS } from "../../lib/limits";
import { peakDb } from "../../lib/microphone";
import type { PatchNode, PatchNodeOf, VoiceGateSettings } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { talkTargets } from "../pushToTalk";
import { reachesArmedTransmitter } from "./airState";
import { GateControls } from "./GateControls";
import { FaceBody, NodeShell } from "./NodeShell";
import { withMicrophoneGate } from "./transmit";
import { type Microphone, useMicrophone } from "./useMicrophone";
import { SEND_MODES } from "./voiceChat";

const KEY_HINT = "Hold to talk, or hold Space. Shift-click to latch, click again to release";
const UNARMED_HINT = "No armed transmitter";
const KEYLESS_HINT = "Sends without the key";

type MicrophoneSettings = PatchNodeOf<"microphone">["data"];

export function MicrophoneFace({ node }: { node: PatchNode }) {
  if (node.kind !== "microphone") {
    return null;
  }
  return <MicrophoneNodeFace node={node} />;
}

function stateOf(microphone: Microphone, settings: MicrophoneSettings): string | undefined {
  if (!microphone.live) {
    return undefined;
  }
  return settings.send === "voice_activity" ? "listening" : "sending";
}

function gateOf(settings: MicrophoneSettings): VoiceGateSettings {
  return {
    mode: settings.send,
    threshold_db: settings.threshold_db,
    hang_ms: settings.hang_ms,
    volume: VOICE_CHAT_LIMITS.volume_seed,
  };
}

function keyHint(armed: boolean, keys: boolean, error: string | null): string {
  if (error !== null) {
    return error;
  }
  if (!armed) {
    return UNARMED_HINT;
  }
  return keys ? KEY_HINT : KEYLESS_HINT;
}

function MicrophoneNodeFace({ node }: { node: PatchNodeOf<"microphone"> }) {
  const workspace = useWorkspaceContext();
  const held =
    workspace.talking && talkTargets(workspace.graph, workspace.selected).includes(node.id);
  const armed = reachesArmedTransmitter(
    workspace.resolved,
    workspace.graph.nodes,
    workspace.deviceSets,
    node.id,
  );
  const keys = node.data.send === "push_to_talk";
  const microphone = useMicrophone(node.id, workspace.socket, {
    send: node.data.send,
    armed,
    held,
  });
  const [latched, setLatched] = useState(false);
  const edit = (next: Partial<VoiceGateSettings>): void =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) => withMicrophoneGate(current, next)),
    }));
  const release = (): void => {
    if (!latched) {
      microphone.unkey();
    }
  };

  return (
    <NodeShell
      node={node}
      title="Microphone"
      category="source"
      subtitle={stateOf(microphone, node.data)}
    >
      <FaceBody>
        <div className="flex items-center gap-2 p-2">
          <Button
            type="button"
            className={`nodrag ${microphone.keyed ? BTN_DANGER_ARMED : BTN}`}
            aria-pressed={microphone.keyed}
            disabled={!armed || !keys}
            title={keyHint(armed, keys, microphone.error)}
            onPointerDown={(event) => {
              if (latched) {
                setLatched(false);
                microphone.unkey();
                return;
              }
              setLatched(event.shiftKey);
              microphone.key();
            }}
            onPointerUp={release}
            onPointerLeave={release}
            onPointerCancel={release}
            onKeyDown={(event) => {
              if (event.key === " " && !event.repeat) {
                microphone.key();
              }
            }}
            onKeyUp={(event) => {
              if (event.key === " ") {
                release();
              }
            }}
          >
            {latched ? "Latched" : "Talk"}
          </Button>
          <span className="min-w-0 flex-1">
            <MeterBar
              label="Microphone level"
              value={microphone.level}
              fill={microphone.error === null ? "bg-accent" : "bg-danger"}
            />
          </span>
        </div>
      </FaceBody>
      <Inspect node={node.id}>
        <InspectSection>
          <Chips inset="none">
            <GateControls
              name="Microphone"
              gate={gateOf(node.data)}
              modes={SEND_MODES}
              levelDb={microphone.live ? peakDb(microphone.level) : undefined}
              open={microphone.live && node.data.send !== "voice_activity"}
              onEdit={edit}
            />
          </Chips>
        </InspectSection>
      </Inspect>
    </NodeShell>
  );
}
