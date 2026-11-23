import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { AudioControls } from "../../components/AudioControls";
import { Button } from "../../components/BaseControls";
import { audioChainActive } from "../../components/channelSettings";
import { FACE_BTN } from "../../components/controls";
import { Chips } from "../../components/face/Chips";
import { formatHz } from "../../components/format";
import { SettingsForm } from "../../components/SettingsForm";
import { SignalRow } from "../../components/SignalRow";
import { settingsSummary } from "../../components/settingsSummary";
import { nodeAction } from "../../lib/api";
import { clearAction, failAction } from "../../lib/refusals";
import type { Settings } from "../../lib/settingsForm";
import type {
  AudioProcessing,
  EncoderDescriptor,
  ParamChoice,
  PatchNode,
  PatchNodeOf,
} from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { nodeOf, patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { AirLabel } from "./AirLabel";
import { airOf, transmitterOf } from "./airState";
import { useBasebandLevel } from "./basebandLevel";
import { FaceState, StateValue } from "./FaceState";
import { FaceBody, FaceFooter, NodeShell } from "./NodeShell";
import { encoderSettings, transmitStatusOf, withEncoderSettings } from "./transmit";

const NO_CHOICES: readonly ParamChoice[] = [];
const NO_MESSAGE: Settings = {};
const SEND_ACTION = "Send";
const WIDTH_KEY = "bandwidth_hz";

export function EncoderFace({ node }: { node: PatchNode }) {
  if (node.kind !== "encoder") {
    return null;
  }
  return <EncoderNodeFace node={node} />;
}

function EncoderNodeFace({ node }: { node: PatchNodeOf<"encoder"> }) {
  const workspace = useWorkspaceContext();
  const descriptor = workspace.context.encoderTypes?.find(
    (type) => type.type_id === node.data.encoder_type,
  );
  const settings = encoderSettings(node.data.settings, descriptor);
  const [message, setMessage] = useState<Settings>(NO_MESSAGE);
  const form = descriptor?.message ?? [];
  const audio = node.data.audio ?? {};
  const voice = descriptor !== undefined && (descriptor.input ?? "audio") === "audio";
  const widthHz = encoderWidthHz(settings, descriptor);
  const transmitter = transmitterOf(workspace.resolved, workspace.graph.nodes, node.id);
  const air = airOf(
    transmitter === null ? null : transmitStatusOf(workspace.deviceSets, transmitter),
  );
  const edit = (change: (current: PatchNode) => PatchNode): void =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, change),
    }));
  const editAudio = (next: AudioProcessing) =>
    edit((current) =>
      current.kind === "encoder" ? { ...current, data: { ...current.data, audio: next } } : current,
    );
  const send = useMutation({
    mutationFn: () => nodeAction(node.id, { action: "send", with: { message } }),
    onSuccess: () => clearAction(node.id, SEND_ACTION),
    onError: (error: Error) => failAction(node.id, SEND_ACTION, error),
  });

  return (
    <NodeShell
      node={node}
      title={descriptor?.name ?? node.data.encoder_type}
      subtitle={air === null ? "Encoder" : <AirLabel air={air} />}
      category="encoder"
      badge={
        <span className="flex gap-1.5">
          {widthHz !== null && <span title="Signal bandwidth">{formatHz(widthHz)}</span>}
          {audioChainActive(audio) && <span title="Audio processing on">FX</span>}
        </span>
      }
    >
      <FaceBody>
        <EncoderState
          node={node.id}
          descriptor={descriptor}
          settings={settings}
          transmitter={transmitter}
        />
      </FaceBody>
      <Inspect node={node.id}>
        {descriptor !== undefined && (descriptor.settings ?? []).length > 0 && (
          <InspectSection>
            <Chips inset="none">
              <SettingsForm
                form={descriptor.settings ?? []}
                settings={settings}
                choices={NO_CHOICES}
                onSettings={(next) => edit((current) => withEncoderSettings(current, next))}
              />
            </Chips>
          </InspectSection>
        )}
        {voice && (
          <InspectSection label="Audio">
            <Chips inset="none">
              <AudioControls audio={audio} onAudio={editAudio} />
            </Chips>
          </InspectSection>
        )}
        {form.length > 0 && (
          <InspectSection label="Message">
            <Chips inset="none">
              <SettingsForm
                form={form}
                settings={message}
                choices={NO_CHOICES}
                onSettings={setMessage}
              />
            </Chips>
          </InspectSection>
        )}
      </Inspect>
      {form.length > 0 && (
        <FaceFooter>
          <Button
            type="button"
            className={FACE_BTN}
            title="Sends this message once"
            disabled={send.isPending}
            onClick={() => send.mutate()}
          >
            Send
          </Button>
        </FaceFooter>
      )}
    </NodeShell>
  );
}

/** The width the header shows: the encoder's own `bandwidth_hz` setting, else its default. */
function encoderWidthHz(settings: Settings, descriptor: EncoderDescriptor | undefined) {
  const own = settings[WIDTH_KEY];
  const width = typeof own === "number" ? own : descriptor?.bandwidth_hz;
  return width !== undefined && Number.isFinite(width) && width > 0 ? width : null;
}

/**
 * The same two rows a decoder shows: what it needs or where it sends in the place of the dial,
 * and the level of the signal it sends.
 */
function EncoderState({
  node,
  descriptor,
  settings,
  transmitter,
}: {
  node: string;
  descriptor: EncoderDescriptor | undefined;
  settings: Settings;
  transmitter: string | null;
}) {
  const workspace = useWorkspaceContext();
  const level = useBasebandLevel(node);
  const need = descriptor === undefined ? "Mode not loaded" : null;
  const target = transmitter === null ? undefined : nodeOf(workspace.graph, transmitter);
  const changed = settingsSummary(descriptor?.settings ?? [], settings).filter(
    (row) => row.key !== WIDTH_KEY,
  );
  return (
    <div className="@container flex flex-col gap-1 p-2">
      <FaceState need={need}>
        <StateValue label="To">{target?.label ?? "-"}</StateValue>
        {changed.map((row) => (
          <StateValue key={row.key} label={row.label}>
            {row.value}
          </StateValue>
        ))}
      </FaceState>
      <SignalRow level={level} />
    </div>
  );
}
