import { useState } from "react";
import { Button } from "../../components/BaseControls";
import { BTN, BTN_DANGER_ARMED } from "../../components/controls";
import { Chips, NumberChip, ToggleChip } from "../../components/face/Chips";
import { MeterBar } from "../../components/face/Meter";
import type { PatchNode, PatchNodeOf } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { talkTargets } from "../pushToTalk";
import { FaceBody, NodeShell } from "./NodeShell";
import { withMicrophone } from "./transmit";
import { type Microphone, useMicrophone } from "./useMicrophone";

const KEY_HINT = "Hold to talk, or hold Space. Shift-click to latch, click again to release";
const MIN_THRESHOLD_DB = -90;
const MAX_HANG_MS = 5_000;

export function MicrophoneFace({ node }: { node: PatchNode }) {
  if (node.kind !== "microphone") {
    return null;
  }
  return <MicrophoneNodeFace node={node} />;
}

function stateOf(microphone: Microphone): string | undefined {
  if (microphone.sending) {
    return "sending";
  }
  return microphone.listening ? "listening" : undefined;
}

function MicrophoneNodeFace({ node }: { node: PatchNodeOf<"microphone"> }) {
  const workspace = useWorkspaceContext();
  const held =
    workspace.talking && talkTargets(workspace.graph, workspace.selected).includes(node.id);
  const microphone = useMicrophone(node.id, workspace.socket, {
    vox: node.data.vox,
    thresholdDb: node.data.threshold_db,
    hangMs: node.data.hang_ms,
    held,
  });
  const [latched, setLatched] = useState(false);
  const edit = (next: Partial<PatchNodeOf<"microphone">["data"]>): void =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) => withMicrophone(current, next)),
    }));
  const release = (): void => {
    if (!latched) {
      microphone.unkey();
    }
  };

  return (
    <NodeShell node={node} title="Microphone" category="source" subtitle={stateOf(microphone)}>
      <FaceBody>
        <div className="flex items-center gap-2 p-2">
          <Button
            type="button"
            className={`nodrag ${microphone.keyed ? BTN_DANGER_ARMED : BTN}`}
            aria-pressed={microphone.keyed}
            title={microphone.error ?? KEY_HINT}
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
        <Chips>
          <ToggleChip
            label="VOX"
            on={node.data.vox}
            title="Key by voice: listens all the time and sends while you are louder than the threshold"
            onChange={(vox) => edit({ vox })}
          />
          {node.data.vox && (
            <>
              <NumberChip
                label="Opens"
                title="Peak level that opens the voice key"
                value={node.data.threshold_db}
                unit="dB"
                min={MIN_THRESHOLD_DB}
                max={0}
                step={1}
                onCommit={(threshold_db) => edit({ threshold_db })}
              />
              <NumberChip
                label="Hang"
                title="How long the voice key stays open after you stop"
                value={node.data.hang_ms}
                unit="ms"
                min={0}
                max={MAX_HANG_MS}
                step={100}
                onCommit={(hang_ms) => edit({ hang_ms })}
              />
            </>
          )}
        </Chips>
      </FaceBody>
    </NodeShell>
  );
}
